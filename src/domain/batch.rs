use std::collections::HashSet;

use thiserror::Error;
use uuid::Uuid;

pub const MAX_BATCH_ITEMS: usize = 50;

/// Exposes the counterpart id a batch is keyed on, so [`validate_batch`] can
/// detect duplicates without every caller passing an extraction closure.
pub trait BatchKey {
    fn key(&self) -> Uuid;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BatchError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },

    #[error("{field} must contain at most {maximum} items, received {received}")]
    TooLarge {
        field: &'static str,
        maximum: usize,
        received: usize,
    },

    #[error("{field} contains duplicate ids")]
    Duplicates { field: &'static str, ids: Vec<Uuid> },

    #[error("{field} contains unknown ids")]
    Unknown { field: &'static str, ids: Vec<Uuid> },
}

pub fn validate_batch<T: BatchKey>(
    field: &'static str,
    items: Vec<T>,
) -> Result<Vec<T>, BatchError> {
    if items.is_empty() {
        return Err(BatchError::Empty { field });
    }
    if items.len() > MAX_BATCH_ITEMS {
        return Err(BatchError::TooLarge {
            field,
            maximum: MAX_BATCH_ITEMS,
            received: items.len(),
        });
    }

    let mut seen = HashSet::with_capacity(items.len());
    let mut duplicates = Vec::new();
    for id in items.iter().map(BatchKey::key) {
        if !seen.insert(id) && !duplicates.contains(&id) {
            duplicates.push(id);
        }
    }

    if !duplicates.is_empty() {
        return Err(BatchError::Duplicates {
            field,
            ids: duplicates,
        });
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::{validate_batch, BatchError, BatchKey, MAX_BATCH_ITEMS};
    use uuid::Uuid;

    /// Carries a non-key field so the duplicate tests prove `validate_batch`
    /// keys off [`BatchKey::key`] rather than whole-item equality.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Item {
        id: Uuid,
        note: String,
    }

    impl Item {
        fn new(id: Uuid, note: &str) -> Self {
            Self {
                id,
                note: note.to_owned(),
            }
        }
    }

    impl BatchKey for Item {
        fn key(&self) -> Uuid {
            self.id
        }
    }

    fn items(count: usize) -> Vec<Item> {
        (0..count)
            .map(|_| Item::new(Uuid::new_v4(), "item"))
            .collect()
    }

    #[test]
    fn batch_at_the_cap_is_accepted() {
        let batch = items(MAX_BATCH_ITEMS);

        assert_eq!(validate_batch("control_ids", batch.clone()), Ok(batch));
    }

    #[test]
    fn batch_over_the_cap_reports_the_limit_and_the_received_count() {
        assert_eq!(
            validate_batch("control_ids", items(MAX_BATCH_ITEMS + 1)),
            Err(BatchError::TooLarge {
                field: "control_ids",
                maximum: 50,
                received: 51,
            })
        );
    }

    #[test]
    fn empty_batch_is_rejected() {
        assert_eq!(
            validate_batch("control_ids", Vec::<Item>::new()),
            Err(BatchError::Empty {
                field: "control_ids"
            })
        );
    }

    #[test]
    fn repeated_ids_are_each_reported_once_in_first_seen_order() {
        let [first, second, third] = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let batch = vec![
            Item::new(third, "a"),
            Item::new(first, "b"),
            Item::new(second, "c"),
            Item::new(first, "d"),
            Item::new(third, "e"),
            Item::new(first, "f"),
        ];

        assert_eq!(
            validate_batch("evidence_ids", batch),
            Err(BatchError::Duplicates {
                field: "evidence_ids",
                ids: vec![first, third],
            })
        );
    }

    #[test]
    fn duplicates_are_detected_through_the_key() {
        let control_id = Uuid::new_v4();
        let batch = vec![
            Item::new(control_id, "covers access review"),
            Item::new(control_id, "covers offboarding"),
        ];

        assert_eq!(
            validate_batch("control_ids", batch),
            Err(BatchError::Duplicates {
                field: "control_ids",
                ids: vec![control_id],
            })
        );
    }
}
