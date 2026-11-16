//! Runtime allocation primitives used by Mosh's low-level memory layer.
//!
//! Allocations own their storage and release it with the exact layout used at
//! creation. Safe byte operations are bounds checked. Raw pointer access is
//! explicitly unsafe because the caller must uphold pointer validity,
//! aliasing, initialization, and lifetime rules.

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::fmt;
use std::ptr::NonNull;

use crate::layout::{compute_layout, LayoutError, LayoutType, TargetDataLayout};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocationError {
    InvalidLayout {
        size: usize,
        align: usize,
    },
    OutOfMemory {
        size: usize,
        align: usize,
    },
    OutOfBounds {
        offset: usize,
        length: usize,
        allocation_size: usize,
    },
}

impl fmt::Display for AllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLayout { size, align } => write!(
                f,
                "invalid allocation layout: size {size}, alignment {align}"
            ),
            Self::OutOfMemory { size, align } => {
                write!(f, "allocation failed: size {size}, alignment {align}")
            }
            Self::OutOfBounds {
                offset,
                length,
                allocation_size,
            } => write!(
                f,
                "memory range {offset}..{} exceeds allocation size {allocation_size}",
                offset.saturating_add(*length)
            ),
        }
    }
}

impl std::error::Error for AllocationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeTypeAllocationError {
    Layout(LayoutError),
    Allocation(AllocationError),
}

impl fmt::Display for RuntimeTypeAllocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => write!(f, "cannot lay out runtime type: {error}"),
            Self::Allocation(error) => write!(f, "cannot allocate runtime type: {error}"),
        }
    }
}

impl std::error::Error for RuntimeTypeAllocationError {}

/// An owned, zero-initialized allocation with a fixed size and alignment.
///
/// Zero-sized allocations use a non-null dangling pointer and are never
/// passed to the global allocator or deallocator.
pub struct Allocation {
    pointer: NonNull<u8>,
    layout: Layout,
    allocated: bool,
}

impl fmt::Debug for Allocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Allocation")
            .field("size", &self.size())
            .field("align", &self.align())
            .field("allocated", &self.allocated)
            .finish()
    }
}

impl Allocation {
    /// Allocates storage using the compiler's target-aware layout for a type.
    pub fn for_type(
        ty: &LayoutType,
        target: TargetDataLayout,
    ) -> Result<Self, RuntimeTypeAllocationError> {
        let layout = compute_layout(ty, target).map_err(RuntimeTypeAllocationError::Layout)?;
        Self::zeroed(layout.size, layout.align).map_err(RuntimeTypeAllocationError::Allocation)
    }

    pub fn zeroed(size: usize, align: usize) -> Result<Self, AllocationError> {
        let layout = Layout::from_size_align(size, align)
            .map_err(|_| AllocationError::InvalidLayout { size, align })?;

        if size == 0 {
            return Ok(Self {
                pointer: NonNull::new(align as *mut u8).expect("validated alignment is nonzero"),
                layout,
                allocated: false,
            });
        }

        // SAFETY: layout was validated above and has non-zero size. The
        // returned allocation is uniquely owned by this value until Drop.
        let raw = unsafe { alloc_zeroed(layout) };
        let pointer = NonNull::new(raw).ok_or(AllocationError::OutOfMemory { size, align })?;

        Ok(Self {
            pointer,
            layout,
            allocated: true,
        })
    }

    pub fn size(&self) -> usize {
        self.layout.size()
    }
    pub fn align(&self) -> usize {
        self.layout.align()
    }
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    fn checked_range(&self, offset: usize, length: usize) -> Result<(), AllocationError> {
        let end = offset
            .checked_add(length)
            .ok_or(AllocationError::OutOfBounds {
                offset,
                length,
                allocation_size: self.size(),
            })?;
        if end > self.size() {
            return Err(AllocationError::OutOfBounds {
                offset,
                length,
                allocation_size: self.size(),
            });
        }
        Ok(())
    }

    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<(), AllocationError> {
        self.checked_range(offset, bytes.len())?;
        if bytes.is_empty() {
            return Ok(());
        }
        // SAFETY: range checked; allocation is live and mutably borrowed;
        // input slice cannot alias this allocation.
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.pointer.as_ptr().add(offset),
                bytes.len(),
            );
        }
        Ok(())
    }

    pub fn read(&self, offset: usize, length: usize) -> Result<Vec<u8>, AllocationError> {
        self.checked_range(offset, length)?;
        let mut result = vec![0; length];
        if length == 0 {
            return Ok(result);
        }
        // SAFETY: range checked, allocation is live, and result is a distinct
        // initialized buffer of exactly length bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.pointer.as_ptr().add(offset),
                result.as_mut_ptr(),
                length,
            );
        }
        Ok(result)
    }

    /// Exposes the raw mutable address. Dereferencing it, creating references
    /// from it, and pointer arithmetic require the caller to uphold Rust's
    /// pointer validity, aliasing, initialization, and lifetime rules.
    ///
    /// # Safety
    /// The caller must not use the pointer after this allocation is dropped,
    /// must preserve alignment and bounds, and must obey aliasing rules.
    pub unsafe fn as_mut_ptr(&mut self) -> *mut u8 {
        self.pointer.as_ptr()
    }

    /// Exposes the raw const address. It must not outlive this allocation.
    pub fn as_ptr(&self) -> *const u8 {
        self.pointer.as_ptr()
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        if self.allocated {
            // SAFETY: pointer came from alloc_zeroed with this exact layout,
            // is uniquely owned, and is deallocated at most once.
            unsafe { dealloc(self.pointer.as_ptr(), self.layout) };
            self.allocated = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_storage_from_compiler_type_layout() {
        let ty = LayoutType::Struct {
            name: "Pair".into(),
            fields: vec![
                crate::layout::StructField {
                    name: "flag".into(),
                    ty: LayoutType::Bool,
                },
                crate::layout::StructField {
                    name: "value".into(),
                    ty: LayoutType::Int { bits: 64 },
                },
            ],
        };
        let allocation = Allocation::for_type(&ty, TargetDataLayout::LINUX_X86_64).unwrap();
        assert_eq!(allocation.size(), 16);
        assert_eq!(allocation.align(), 8);
    }

    #[test]
    fn creates_zero_initialized_aligned_memory() {
        let allocation = Allocation::zeroed(32, 16).unwrap();
        assert_eq!(allocation.size(), 32);
        assert_eq!(allocation.align(), 16);
        assert_eq!((allocation.as_ptr() as usize) % 16, 0);
        assert_eq!(allocation.read(0, 32).unwrap(), vec![0; 32]);
    }

    #[test]
    fn writes_and_reads_bytes_at_offsets() {
        let mut allocation = Allocation::zeroed(8, 8).unwrap();
        allocation.write(2, &[10, 20, 30]).unwrap();
        assert_eq!(
            allocation.read(0, 8).unwrap(),
            vec![0, 0, 10, 20, 30, 0, 0, 0]
        );
    }

    #[test]
    fn rejects_ranges_beyond_allocation() {
        let mut allocation = Allocation::zeroed(4, 1).unwrap();
        assert!(matches!(
            allocation.write(3, &[1, 2]),
            Err(AllocationError::OutOfBounds { .. })
        ));
        assert!(matches!(
            allocation.read(usize::MAX, 2),
            Err(AllocationError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn accepts_zero_sized_allocation() {
        let allocation = Allocation::zeroed(0, 8).unwrap();
        assert!(allocation.is_empty());
        assert!(allocation.read(0, 0).unwrap().is_empty());
        assert!(!allocation.as_ptr().is_null());
    }

    #[test]
    fn rejects_invalid_alignment() {
        assert!(matches!(
            Allocation::zeroed(8, 3),
            Err(AllocationError::InvalidLayout { .. })
        ));
        assert!(matches!(
            Allocation::zeroed(8, 0),
            Err(AllocationError::InvalidLayout { .. })
        ));
    }
}
