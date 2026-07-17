use crate::validation::Validation;

use super::DomainError;

pub fn required_text(field: &'static str, value: String) -> Validation<String, DomainError> {
    if value.trim().is_empty() {
        return Validation::invalid(DomainError::EmptyRequiredText { field });
    }

    Validation::valid(value)
}

pub fn optional_text(
    field: &'static str,
    value: Option<String>,
    maximum: usize,
) -> Validation<Option<String>, DomainError> {
    let Some(value) = value else {
        return Validation::valid(None);
    };
    let value = value.trim().to_owned();

    if value.is_empty() {
        return Validation::invalid(DomainError::BlankOptionalText { field });
    }
    if value.chars().count() > maximum {
        return Validation::invalid(DomainError::OptionalTextTooLong { field, maximum });
    }

    Validation::valid(Some(value))
}

pub fn validate_submission_filename(value: String) -> Validation<String, DomainError> {
    let mut errors = Vec::new();

    if value.trim().is_empty() {
        errors.push(DomainError::EmptySubmissionFilename);
    }
    if value.len() > 255 {
        errors.push(DomainError::SubmissionFilenameTooLong);
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'_' | b'-' | b'(' | b')')
    }) {
        errors.push(DomainError::InvalidSubmissionFilenameCharacters);
    }
    if matches!(value.as_str(), "." | "..") {
        errors.push(DomainError::ReservedSubmissionFilename);
    }

    if errors.is_empty() {
        Validation::valid(value)
    } else {
        Validation::invalid_many(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::{optional_text, required_text, validate_submission_filename};
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
    fn optional_text_accepts_omitted_and_trims_present_values() {
        assert_eq!(optional_text("summary", None, 500).into_result(), Ok(None));
        assert_eq!(
            optional_text("summary", Some("  Quarterly review  ".to_owned()), 500).into_result(),
            Ok(Some("Quarterly review".to_owned()))
        );
    }

    #[test]
    fn optional_text_rejects_blank_values() {
        assert_eq!(
            optional_text("summary", Some(" \t\n ".to_owned()), 500).into_result(),
            Err(vec![DomainError::BlankOptionalText { field: "summary" }])
        );
    }

    #[test]
    fn optional_text_limits_unicode_characters_after_trimming() {
        assert_eq!(
            optional_text("summary", Some("é".repeat(500)), 500).into_result(),
            Ok(Some("é".repeat(500)))
        );
        assert_eq!(
            optional_text("summary", Some(format!(" {} ", "é".repeat(501))), 500).into_result(),
            Err(vec![DomainError::OptionalTextTooLong {
                field: "summary",
                maximum: 500,
            }])
        );
    }

    #[test]
    fn submission_filename_accepts_portable_ascii_and_preserves_it() {
        let filename = "  Quarterly evidence (final).csv  ".to_owned();
        let maximum_length_filename = "a".repeat(255);

        assert_eq!(
            validate_submission_filename(filename.clone()).into_result(),
            Ok(filename)
        );
        assert_eq!(
            validate_submission_filename(maximum_length_filename.clone()).into_result(),
            Ok(maximum_length_filename)
        );
    }

    #[test]
    fn submission_filename_rejects_blank_values() {
        assert_eq!(
            validate_submission_filename("   ".to_owned()).into_result(),
            Err(vec![DomainError::EmptySubmissionFilename])
        );
    }

    #[test]
    fn submission_filename_rejects_names_over_255_bytes() {
        assert_eq!(
            validate_submission_filename("a".repeat(256)).into_result(),
            Err(vec![DomainError::SubmissionFilenameTooLong])
        );
    }

    #[test]
    fn submission_filename_rejects_separators_quotes_unicode_and_controls() {
        for filename in [
            "path/file.txt",
            r"path\file.txt",
            "file\"name.txt",
            "résumé.txt",
            "file\tname.txt",
        ] {
            assert_eq!(
                validate_submission_filename(filename.to_owned()).into_result(),
                Err(vec![DomainError::InvalidSubmissionFilenameCharacters]),
                "{filename}"
            );
        }
    }

    #[test]
    fn submission_filename_rejects_dot_and_dot_dot() {
        for filename in [".", ".."] {
            assert_eq!(
                validate_submission_filename(filename.to_owned()).into_result(),
                Err(vec![DomainError::ReservedSubmissionFilename]),
                "{filename}"
            );
        }
    }

    #[test]
    fn submission_filename_accumulates_independent_errors() {
        assert_eq!(
            validate_submission_filename("\t".repeat(256)).into_result(),
            Err(vec![
                DomainError::EmptySubmissionFilename,
                DomainError::SubmissionFilenameTooLong,
                DomainError::InvalidSubmissionFilenameCharacters,
            ])
        );
    }
}
