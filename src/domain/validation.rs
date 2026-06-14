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

pub fn validate_attachment_filename(value: String) -> Validation<String, DomainError> {
    let mut errors = Vec::new();

    if value.trim().is_empty() {
        errors.push(DomainError::EmptyAttachmentFilename);
    }
    if value.len() > 255 {
        errors.push(DomainError::AttachmentFilenameTooLong);
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'_' | b'-' | b'(' | b')')
    }) {
        errors.push(DomainError::InvalidAttachmentFilenameCharacters);
    }
    if matches!(value.as_str(), "." | "..") {
        errors.push(DomainError::ReservedAttachmentFilename);
    }

    if errors.is_empty() {
        Validation::valid(value)
    } else {
        Validation::invalid_many(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::{required_text, validate_attachment_filename, validate_freshness_window_days};
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

    #[test]
    fn attachment_filename_accepts_portable_ascii_and_preserves_it() {
        let filename = "  Quarterly evidence (final).csv  ".to_owned();
        let maximum_length_filename = "a".repeat(255);

        assert_eq!(
            validate_attachment_filename(filename.clone()).into_result(),
            Ok(filename)
        );
        assert_eq!(
            validate_attachment_filename(maximum_length_filename.clone()).into_result(),
            Ok(maximum_length_filename)
        );
    }

    #[test]
    fn attachment_filename_rejects_blank_values() {
        assert_eq!(
            validate_attachment_filename("   ".to_owned()).into_result(),
            Err(vec![DomainError::EmptyAttachmentFilename])
        );
    }

    #[test]
    fn attachment_filename_rejects_names_over_255_bytes() {
        assert_eq!(
            validate_attachment_filename("a".repeat(256)).into_result(),
            Err(vec![DomainError::AttachmentFilenameTooLong])
        );
    }

    #[test]
    fn attachment_filename_rejects_separators_quotes_unicode_and_controls() {
        for filename in [
            "path/file.txt",
            r"path\file.txt",
            "file\"name.txt",
            "résumé.txt",
            "file\tname.txt",
        ] {
            assert_eq!(
                validate_attachment_filename(filename.to_owned()).into_result(),
                Err(vec![DomainError::InvalidAttachmentFilenameCharacters]),
                "{filename}"
            );
        }
    }

    #[test]
    fn attachment_filename_rejects_dot_and_dot_dot() {
        for filename in [".", ".."] {
            assert_eq!(
                validate_attachment_filename(filename.to_owned()).into_result(),
                Err(vec![DomainError::ReservedAttachmentFilename]),
                "{filename}"
            );
        }
    }

    #[test]
    fn attachment_filename_accumulates_independent_errors() {
        assert_eq!(
            validate_attachment_filename("\t".repeat(256)).into_result(),
            Err(vec![
                DomainError::EmptyAttachmentFilename,
                DomainError::AttachmentFilenameTooLong,
                DomainError::InvalidAttachmentFilenameCharacters,
            ])
        );
    }
}
