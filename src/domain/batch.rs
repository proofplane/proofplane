use std::collections::HashSet;

use thiserror::Error;
use uuid::Uuid;

use crate::validation::Validation;

pub const MAX_BATCH_ITEMS: usize = 50;

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
}

/// Validates the shape of a batch before any database work happens.
///
/// `key` extracts the counterpart id from each item so one helper can
/// be used both for simple lists of IDs as well as more complex batches
/// of mapping targets.
pub fn validate_batch<T>(
    field: &'static str,
    items: Vec<T>,
    key: impl Fn(&T) -> Uuid,
) -> Validation<Vec<T>, BatchError> {
    if items.is_empty() {
        return Validation::invalid(BatchError::Empty { field });
    }
    if items.len() > MAX_BATCH_ITEMS {
        return Validation::invalid(BatchError::TooLarge {
            field,
            maximum: MAX_BATCH_ITEMS,
            received: items.len(),
        });
    }

    let mut seen = HashSet::with_capacity(items.len());
    let mut duplicates = Vec::new();
    for id in items.iter().map(&key) {
        if !seen.insert(id) && !duplicates.contains(&id) {
            duplicates.push(id);
        }
    }

    if !duplicates.is_empty() {
        return Validation::invalid(BatchError::Duplicates {
            field,
            ids: duplicates,
        });
    }

    Validation::valid(items)
}

#[cfg(test)]
mod tests {
    use super::{validate_batch, BatchError, MAX_BATCH_ITEMS};
    use uuid::Uuid;

    fn ids(count: usize) -> Vec<Uuid> {
        (0..count).map(|_| Uuid::new_v4()).collect()
    }

    #[test]
    fn batch_at_the_cap_is_accepted() {
        let items = ids(MAX_BATCH_ITEMS);

        assert_eq!(
            validate_batch("control_ids", items.clone(), |id| *id).into_result(),
            Ok(items)
        );
    }

    #[test]
    fn batch_over_the_cap_reports_the_limit_and_the_received_count() {
        assert_eq!(
            validate_batch("control_ids", ids(MAX_BATCH_ITEMS + 1), |id| *id).into_result(),
            Err(vec![BatchError::TooLarge {
                field: "control_ids",
                maximum: 50,
                received: 51,
            }])
        );
    }

    #[test]
    fn empty_batch_is_rejected() {
        assert_eq!(
            validate_batch("control_ids", Vec::<Uuid>::new(), |id| *id).into_result(),
            Err(vec![BatchError::Empty {
                field: "control_ids"
            }])
        );
    }

    #[test]
    fn repeated_ids_are_each_reported_once_in_first_seen_order() {
        let [first, second, third] = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        let items = vec![third, first, second, first, third, first];

        assert_eq!(
            validate_batch("evidence_ids", items, |id| *id).into_result(),
            Err(vec![BatchError::Duplicates {
                field: "evidence_ids",
                ids: vec![first, third],
            }])
        );
    }

    #[test]
    fn duplicates_are_detected_through_the_key_extractor() {
        let control_id = Uuid::new_v4();
        let items = vec![
            (control_id, "covers access review".to_owned()),
            (control_id, "covers offboarding".to_owned()),
        ];

        assert_eq!(
            validate_batch("control_ids", items, |(id, _)| *id).into_result(),
            Err(vec![BatchError::Duplicates {
                field: "control_ids",
                ids: vec![control_id],
            }])
        );
    }
}
