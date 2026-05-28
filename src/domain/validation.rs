use crate::validation::Validation;

use super::DomainError;

pub fn required_text(field: &'static str, value: String) -> Validation<String, DomainError> {
    if value.trim().is_empty() {
        return Validation::invalid(DomainError::EmptyRequiredText { field });
    }

    Validation::valid(value)
}

pub fn validate_freshness_window_days(value: Option<i32>) -> Validation<Option<i32>, DomainError> {
    match value {
        Some(days) if days <= 0 => Validation::invalid(DomainError::InvalidFreshnessWindowDays),
        _ => Validation::valid(value),
    }
}

#[cfg(test)]
mod tests {
    use super::{required_text, validate_freshness_window_days};
    use crate::domain::DomainError;

    #[test]
    fn required_text_rejects_blank_values() {
        assert_eq!(
            required_text("title", " \t\n".to_owned()).into_result(),
            Err(vec![DomainError::EmptyRequiredText { field: "title" }])
        );
    }

    #[test]
    fn required_text_preserves_original_value_when_present() {
        assert_eq!(
            required_text("title", "  Quarterly review  ".to_owned()).into_result(),
            Ok("  Quarterly review  ".to_owned())
        );
    }

    #[test]
    fn freshness_window_days_must_be_positive_when_present() {
        assert_eq!(
            validate_freshness_window_days(Some(0)).into_result(),
            Err(vec![DomainError::InvalidFreshnessWindowDays])
        );
        assert_eq!(
            validate_freshness_window_days(Some(-1)).into_result(),
            Err(vec![DomainError::InvalidFreshnessWindowDays])
        );
    }

    #[test]
    fn freshness_window_days_allows_absent_or_positive_values() {
        assert_eq!(validate_freshness_window_days(None).into_result(), Ok(None));
        assert_eq!(
            validate_freshness_window_days(Some(30)).into_result(),
            Ok(Some(30))
        );
    }
}
