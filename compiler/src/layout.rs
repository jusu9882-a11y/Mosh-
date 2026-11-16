//! Target-aware storage layout for Mosh aggregate and low-level types.
//!
//! This module computes storage size, ABI alignment, array stride, and struct
//! field offsets. It deliberately does not infer a target ABI: callers must
//! supply the pointer size/alignment for the target being compiled.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutType {
    Unit,
    Bool,
    Int {
        bits: u16,
    },
    Float {
        bits: u16,
    },
    Pointer,
    Array {
        element: Box<LayoutType>,
        length: usize,
    },
    Struct {
        name: String,
        fields: Vec<StructField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub name: String,
    pub ty: LayoutType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetDataLayout {
    pub pointer_size: usize,
    pub pointer_align: usize,
}

impl TargetDataLayout {
    pub const fn new(pointer_size: usize, pointer_align: usize) -> Self {
        Self {
            pointer_size,
            pointer_align,
        }
    }

    /// The conventional 64-bit layout used by the current Linux x86-64 preview.
    /// Other targets must provide their own ABI layout explicitly.
    pub const LINUX_X86_64: Self = Self::new(8, 8);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayout {
    pub name: String,
    pub offset: usize,
    pub layout: TypeLayoutInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeLayoutInfo {
    pub size: usize,
    pub align: usize,
    pub fields: Vec<FieldLayout>,
    /// For arrays, the distance in bytes between adjacent elements.
    pub stride: Option<usize>,
}

impl TypeLayoutInfo {
    fn scalar(size: usize, align: usize) -> Self {
        Self {
            size,
            align,
            fields: Vec::new(),
            stride: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    InvalidPointerLayout { size: usize, align: usize },
    UnsupportedScalarWidth { kind: &'static str, bits: u16 },
    DuplicateField { structure: String, field: String },
    SizeOverflow,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPointerLayout { size, align } => write!(
                f,
                "invalid target pointer layout: size {size}, alignment {align}"
            ),
            Self::UnsupportedScalarWidth { kind, bits } => {
                write!(
                    f,
                    "unsupported {kind} width {bits}; expected 8, 16, 32, 64, or 128"
                )
            }
            Self::DuplicateField { structure, field } => {
                write!(f, "duplicate field '{field}' in structure '{structure}'")
            }
            Self::SizeOverflow => f.write_str("type layout exceeds the target addressable size"),
        }
    }
}

impl std::error::Error for LayoutError {}

pub fn compute_layout(
    ty: &LayoutType,
    target: TargetDataLayout,
) -> Result<TypeLayoutInfo, LayoutError> {
    validate_target(target)?;
    layout_inner(ty, target)
}

fn validate_target(target: TargetDataLayout) -> Result<(), LayoutError> {
    if target.pointer_size == 0
        || target.pointer_align == 0
        || !target.pointer_align.is_power_of_two()
    {
        return Err(LayoutError::InvalidPointerLayout {
            size: target.pointer_size,
            align: target.pointer_align,
        });
    }
    Ok(())
}

fn scalar_layout(kind: &'static str, bits: u16) -> Result<TypeLayoutInfo, LayoutError> {
    if !matches!(bits, 8 | 16 | 32 | 64 | 128) {
        return Err(LayoutError::UnsupportedScalarWidth { kind, bits });
    }
    let size = usize::from(bits / 8);
    Ok(TypeLayoutInfo::scalar(size, size))
}

fn layout_inner(ty: &LayoutType, target: TargetDataLayout) -> Result<TypeLayoutInfo, LayoutError> {
    match ty {
        LayoutType::Unit => Ok(TypeLayoutInfo::scalar(0, 1)),
        LayoutType::Bool => Ok(TypeLayoutInfo::scalar(1, 1)),
        LayoutType::Int { bits } => scalar_layout("integer", *bits),
        LayoutType::Float { bits } => scalar_layout("floating-point", *bits),
        LayoutType::Pointer => Ok(TypeLayoutInfo::scalar(
            target.pointer_size,
            target.pointer_align,
        )),
        LayoutType::Array { element, length } => {
            let element_layout = layout_inner(element, target)?;
            let stride = align_up(element_layout.size, element_layout.align)?;
            let size = stride
                .checked_mul(*length)
                .ok_or(LayoutError::SizeOverflow)?;
            Ok(TypeLayoutInfo {
                size,
                align: element_layout.align,
                fields: Vec::new(),
                stride: Some(stride),
            })
        }
        LayoutType::Struct { name, fields } => {
            let mut seen = std::collections::HashSet::with_capacity(fields.len());
            let mut offset = 0usize;
            let mut struct_align = 1usize;
            let mut field_layouts = Vec::with_capacity(fields.len());

            for field in fields {
                if !seen.insert(field.name.as_str()) {
                    return Err(LayoutError::DuplicateField {
                        structure: name.clone(),
                        field: field.name.clone(),
                    });
                }
                let field_layout = layout_inner(&field.ty, target)?;
                offset = align_up(offset, field_layout.align)?;
                field_layouts.push(FieldLayout {
                    name: field.name.clone(),
                    offset,
                    layout: field_layout.clone(),
                });
                offset = offset
                    .checked_add(field_layout.size)
                    .ok_or(LayoutError::SizeOverflow)?;
                struct_align = struct_align.max(field_layout.align);
            }

            Ok(TypeLayoutInfo {
                size: align_up(offset, struct_align)?,
                align: struct_align,
                fields: field_layouts,
                stride: None,
            })
        }
    }
}

fn align_up(value: usize, align: usize) -> Result<usize, LayoutError> {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .map(|rounded| rounded & !(align - 1))
        .ok_or(LayoutError::SizeOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, ty: LayoutType) -> StructField {
        StructField {
            name: name.into(),
            ty,
        }
    }

    #[test]
    fn lays_out_scalar_and_pointer_types_for_explicit_target() {
        let target = TargetDataLayout::LINUX_X86_64;
        assert_eq!(
            compute_layout(&LayoutType::Int { bits: 64 }, target).unwrap(),
            TypeLayoutInfo::scalar(8, 8)
        );
        assert_eq!(
            compute_layout(&LayoutType::Pointer, target).unwrap(),
            TypeLayoutInfo::scalar(8, 8)
        );
        assert_eq!(
            compute_layout(&LayoutType::Bool, target).unwrap(),
            TypeLayoutInfo::scalar(1, 1)
        );
    }

    #[test]
    fn computes_struct_field_offsets_and_tail_padding() {
        let ty = LayoutType::Struct {
            name: "Record".into(),
            fields: vec![
                field("tag", LayoutType::Bool),
                field("count", LayoutType::Int { bits: 64 }),
                field("small", LayoutType::Int { bits: 16 }),
            ],
        };
        let layout = compute_layout(&ty, TargetDataLayout::LINUX_X86_64).unwrap();
        assert_eq!((layout.size, layout.align), (24, 8));
        assert_eq!(
            layout
                .fields
                .iter()
                .map(|field| field.offset)
                .collect::<Vec<_>>(),
            vec![0, 8, 16]
        );
    }

    #[test]
    fn computes_array_stride_and_total_size() {
        let ty = LayoutType::Array {
            element: Box::new(LayoutType::Int { bits: 32 }),
            length: 5,
        };
        let layout = compute_layout(&ty, TargetDataLayout::LINUX_X86_64).unwrap();
        assert_eq!((layout.size, layout.align, layout.stride), (20, 4, Some(4)));
    }

    #[test]
    fn rejects_duplicate_structure_fields() {
        let ty = LayoutType::Struct {
            name: "Duplicate".into(),
            fields: vec![
                field("value", LayoutType::Bool),
                field("value", LayoutType::Int { bits: 32 }),
            ],
        };
        assert_eq!(
            compute_layout(&ty, TargetDataLayout::LINUX_X86_64),
            Err(LayoutError::DuplicateField {
                structure: "Duplicate".into(),
                field: "value".into(),
            })
        );
    }

    #[test]
    fn rejects_invalid_target_and_scalar_widths() {
        assert!(matches!(
            compute_layout(&LayoutType::Pointer, TargetDataLayout::new(8, 3)),
            Err(LayoutError::InvalidPointerLayout { .. })
        ));
        assert!(matches!(
            compute_layout(
                &LayoutType::Int { bits: 24 },
                TargetDataLayout::LINUX_X86_64
            ),
            Err(LayoutError::UnsupportedScalarWidth { .. })
        ));
    }

    #[test]
    fn rejects_array_size_overflow() {
        let ty = LayoutType::Array {
            element: Box::new(LayoutType::Int { bits: 128 }),
            length: usize::MAX,
        };
        assert_eq!(
            compute_layout(&ty, TargetDataLayout::LINUX_X86_64),
            Err(LayoutError::SizeOverflow)
        );
    }
}
