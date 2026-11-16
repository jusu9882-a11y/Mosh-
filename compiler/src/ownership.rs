//! Ownership and borrowing state for Mosh places.
//!
//! The engine is deliberately independent of parsing: the frontend translates
//! typed places and lexical borrow lifetimes into these operations. A place is
//! a root binding followed by field/index projections. Overlapping projections
//! are treated conservatively when an index is not statically known.

use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Projection {
    Field(String),
    ConstantIndex(usize),
    DynamicIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub root: String,
    pub projections: Vec<Projection>,
}

impl Place {
    pub fn new(root: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            projections: Vec::new(),
        }
    }

    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.projections.push(Projection::Field(name.into()));
        self
    }

    pub fn index(mut self, index: usize) -> Self {
        self.projections.push(Projection::ConstantIndex(index));
        self
    }

    pub fn dynamic_index(mut self) -> Self {
        self.projections.push(Projection::DynamicIndex);
        self
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        self.root == other.root
            && self.projections.len() <= other.projections.len()
            && self.projections == other.projections[..self.projections.len()]
    }

    fn overlaps(&self, other: &Self) -> bool {
        if self.root != other.root {
            return false;
        }
        for (left, right) in self.projections.iter().zip(&other.projections) {
            match (left, right) {
                (Projection::Field(a), Projection::Field(b)) if a == b => {}
                (Projection::ConstantIndex(a), Projection::ConstantIndex(b)) if a == b => {}
                (Projection::DynamicIndex, Projection::ConstantIndex(_))
                | (Projection::ConstantIndex(_), Projection::DynamicIndex)
                | (Projection::DynamicIndex, Projection::DynamicIndex) => {}
                _ => return false,
            }
        }
        // A prefix denotes an aggregate containing the more specific place.
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowKind {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveBorrow {
    place: Place,
    kind: BorrowKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipError {
    UseAfterMove(Place),
    MoveWhileBorrowed(Place),
    ReadWhileExclusivelyBorrowed(Place),
    WriteWhileBorrowed(Place),
    ConflictingBorrow { place: Place, requested: BorrowKind },
    UnknownBorrow(u64),
}

impl fmt::Display for OwnershipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UseAfterMove(place) => write!(f, "use of moved value '{}'", place.root),
            Self::MoveWhileBorrowed(place) => {
                write!(f, "cannot move '{}' while it is borrowed", place.root)
            }
            Self::ReadWhileExclusivelyBorrowed(place) => write!(
                f,
                "cannot read '{}' while it is exclusively borrowed",
                place.root
            ),
            Self::WriteWhileBorrowed(place) => {
                write!(f, "cannot write '{}' while it is borrowed", place.root)
            }
            Self::ConflictingBorrow {
                place,
                requested: BorrowKind::Shared,
            } => write!(
                f,
                "cannot create a shared borrow of '{}' while it is exclusively borrowed",
                place.root
            ),
            Self::ConflictingBorrow {
                place,
                requested: BorrowKind::Exclusive,
            } => write!(
                f,
                "cannot create an exclusive borrow of '{}' while it is borrowed",
                place.root
            ),
            Self::UnknownBorrow(id) => write!(f, "unknown or already released borrow {id}"),
        }
    }
}

impl std::error::Error for OwnershipError {}

/// Per-function ownership state. Borrow IDs are monotonically allocated and
/// released when the frontend reaches the end of the corresponding lexical
/// borrow region.
#[derive(Debug, Default)]
pub struct OwnershipState {
    moved: HashSet<Place>,
    reinitialized: HashSet<Place>,
    borrows: HashMap<u64, ActiveBorrow>,
    next_borrow: u64,
}

impl OwnershipState {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_live(&self, place: &Place) -> Result<(), OwnershipError> {
        if self.moved.iter().any(|moved| {
            moved.overlaps(place)
                && !(moved.is_prefix_of(place)
                    && self
                        .reinitialized
                        .iter()
                        .any(|initialized| {
                            moved.is_prefix_of(initialized) && initialized.is_prefix_of(place)
                        }))
        }) {
            Err(OwnershipError::UseAfterMove(place.clone()))
        } else {
            Ok(())
        }
    }

    pub fn read(&self, place: &Place) -> Result<(), OwnershipError> {
        self.ensure_live(place)?;
        if self
            .borrows
            .values()
            .any(|borrow| borrow.kind == BorrowKind::Exclusive && borrow.place.overlaps(place))
        {
            return Err(OwnershipError::ReadWhileExclusivelyBorrowed(place.clone()));
        }
        Ok(())
    }

    pub fn move_out(&mut self, place: Place) -> Result<(), OwnershipError> {
        self.ensure_live(&place)?;
        if self
            .borrows
            .values()
            .any(|borrow| borrow.place.overlaps(&place))
        {
            return Err(OwnershipError::MoveWhileBorrowed(place));
        }
        self.reinitialized
            .retain(|initialized| !place.is_prefix_of(initialized));
        self.moved.insert(place);
        Ok(())
    }

    pub fn assign(&mut self, place: Place) -> Result<(), OwnershipError> {
        if self
            .borrows
            .values()
            .any(|borrow| borrow.place.overlaps(&place))
        {
            return Err(OwnershipError::WriteWhileBorrowed(place));
        }
        // Reinitializing an aggregate clears moved descendants. Reinitializing
        // a field inside a moved aggregate records only that field as live;
        // it must not accidentally revive the aggregate's other fields.
        self.moved.retain(|moved| !place.is_prefix_of(moved));
        self.reinitialized
            .retain(|initialized| !place.is_prefix_of(initialized));
        if self.moved.iter().any(|moved| moved.is_prefix_of(&place)) {
            self.reinitialized.insert(place);
        }
        Ok(())
    }

    pub fn borrow(&mut self, place: Place, kind: BorrowKind) -> Result<u64, OwnershipError> {
        self.ensure_live(&place)?;
        let conflict = self.borrows.values().any(|active| {
            active.place.overlaps(&place)
                && (kind == BorrowKind::Exclusive || active.kind == BorrowKind::Exclusive)
        });
        if conflict {
            return Err(OwnershipError::ConflictingBorrow {
                place,
                requested: kind,
            });
        }
        let id = self.next_borrow;
        self.next_borrow = self
            .next_borrow
            .checked_add(1)
            .expect("borrow ID space exhausted");
        self.borrows.insert(id, ActiveBorrow { place, kind });
        Ok(id)
    }

    pub fn release(&mut self, id: u64) -> Result<(), OwnershipError> {
        self.borrows
            .remove(&id)
            .map(|_| ())
            .ok_or(OwnershipError::UnknownBorrow(id))
    }

    pub fn is_moved(&self, place: &Place) -> bool {
        self.ensure_live(place).is_err()
    }

    pub fn active_borrow_count(&self) -> usize {
        self.borrows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_rejects_later_reads_and_moves() {
        let mut state = OwnershipState::new();
        let value = Place::new("value");
        state.move_out(value.clone()).unwrap();
        assert_eq!(
            state.read(&value),
            Err(OwnershipError::UseAfterMove(value.clone()))
        );
        assert_eq!(
            state.move_out(value.clone()),
            Err(OwnershipError::UseAfterMove(value))
        );
    }

    #[test]
    fn reinitialization_restores_a_moved_place() {
        let mut state = OwnershipState::new();
        let value = Place::new("value");
        state.move_out(value.clone()).unwrap();
        state.assign(value.clone()).unwrap();
        assert!(state.read(&value).is_ok());
    }

    #[test]
    fn disjoint_struct_fields_can_be_moved_independently() {
        let mut state = OwnershipState::new();
        let left = Place::new("pair").field("left");
        let right = Place::new("pair").field("right");
        state.move_out(left.clone()).unwrap();
        assert!(state.read(&right).is_ok());
        assert!(state.read(&Place::new("pair")).is_err());
    }

    #[test]
    fn reinitializing_one_field_does_not_restore_other_fields_of_a_moved_aggregate() {
        let mut state = OwnershipState::new();
        let aggregate = Place::new("pair");
        let left = aggregate.clone().field("left");
        let right = aggregate.clone().field("right");

        state.move_out(aggregate.clone()).unwrap();
        state.assign(left.clone()).unwrap();

        assert!(state.read(&left).is_ok());
        assert!(state.read(&right).is_err());
        assert!(state.read(&aggregate).is_err());
        assert!(state.is_moved(&right));
        assert!(!state.is_moved(&left));
    }

    #[test]
    fn shared_borrows_coexist_but_block_moves() {
        let mut state = OwnershipState::new();
        let place = Place::new("value");
        let first = state.borrow(place.clone(), BorrowKind::Shared).unwrap();
        let second = state.borrow(place.clone(), BorrowKind::Shared).unwrap();
        assert_eq!(
            state.move_out(place.clone()),
            Err(OwnershipError::MoveWhileBorrowed(place.clone()))
        );
        state.release(first).unwrap();
        state.release(second).unwrap();
        assert!(state.move_out(place).is_ok());
    }

    #[test]
    fn exclusive_borrow_blocks_reads_writes_and_other_borrows() {
        let mut state = OwnershipState::new();
        let place = Place::new("value");
        let borrow = state.borrow(place.clone(), BorrowKind::Exclusive).unwrap();
        assert_eq!(
            state.read(&place),
            Err(OwnershipError::ReadWhileExclusivelyBorrowed(place.clone()))
        );
        assert_eq!(
            state.assign(place.clone()),
            Err(OwnershipError::WriteWhileBorrowed(place.clone()))
        );
        assert!(matches!(
            state.borrow(place.clone(), BorrowKind::Shared),
            Err(OwnershipError::ConflictingBorrow { .. })
        ));
        state.release(borrow).unwrap();
        assert!(state.read(&place).is_ok());
    }

    #[test]
    fn dynamic_indices_overlap_all_elements_conservatively() {
        let mut state = OwnershipState::new();
        let dynamic = Place::new("items").dynamic_index();
        let fixed = Place::new("items").index(3);
        let borrow = state.borrow(dynamic, BorrowKind::Exclusive).unwrap();
        assert!(matches!(
            state.read(&fixed),
            Err(OwnershipError::ReadWhileExclusivelyBorrowed(_))
        ));
        state.release(borrow).unwrap();
        assert!(state.read(&fixed).is_ok());
    }

    #[test]
    fn unrelated_roots_do_not_conflict() {
        let mut state = OwnershipState::new();
        state
            .borrow(Place::new("first"), BorrowKind::Exclusive)
            .unwrap();
        assert!(state.read(&Place::new("second")).is_ok());
    }

    #[test]
    fn releasing_a_borrow_twice_is_reported() {
        let mut state = OwnershipState::new();
        let id = state
            .borrow(Place::new("value"), BorrowKind::Shared)
            .unwrap();
        state.release(id).unwrap();
        assert_eq!(state.release(id), Err(OwnershipError::UnknownBorrow(id)));
    }
}
