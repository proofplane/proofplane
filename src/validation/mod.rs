#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation<T, E> {
    Valid(T),
    Invalid(Vec<E>),
}

impl<T, E> Validation<T, E> {
    pub fn valid(value: T) -> Self {
        Self::Valid(value)
    }

    pub fn invalid(error: E) -> Self {
        Self::Invalid(vec![error])
    }

    pub fn invalid_many(errors: Vec<E>) -> Self {
        Self::Invalid(errors)
    }

    pub fn into_result(self) -> Result<T, Vec<E>> {
        match self {
            Self::Valid(value) => Ok(value),
            Self::Invalid(errors) => Err(errors),
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Validation<U, E> {
        match self {
            Self::Valid(value) => Validation::Valid(f(value)),
            Self::Invalid(errors) => Validation::Invalid(errors),
        }
    }

    pub fn and_then<U>(self, f: impl FnOnce(T) -> Validation<U, E>) -> Validation<U, E> {
        match self {
            Self::Valid(value) => f(value),
            Self::Invalid(errors) => Validation::Invalid(errors),
        }
    }

    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid(_))
    }

    pub fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }
}

#[macro_export]
macro_rules! validate {
    (
        $(
            $name:ident <- $validation:expr,
        )+
        => $value:expr $(,)?
    ) => {{
        let mut __validation_errors = Vec::new();
        $(
            let mut $name = None;
            match $validation {
                $crate::validation::Validation::Valid(__validation_value) => {
                    $name = Some(__validation_value);
                }
                $crate::validation::Validation::Invalid(mut __binding_errors) => {
                    __validation_errors.append(&mut __binding_errors);
                }
            }
        )+

        if __validation_errors.is_empty() {
            $(
                let $name = $name.expect("valid validation binding should be present");
            )+
            $crate::validation::Validation::valid($value)
        } else {
            $crate::validation::Validation::Invalid(__validation_errors)
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::Validation;
    use std::cell::Cell;

    #[test]
    fn valid_value_converts_to_ok() {
        assert_eq!(Validation::<_, &str>::valid(1).into_result(), Ok(1));
    }

    #[test]
    fn invalid_value_converts_to_error_list() {
        assert_eq!(
            Validation::<u8, _>::invalid("name").into_result(),
            Err(vec!["name"])
        );
    }

    #[test]
    fn invalid_many_preserves_multiple_errors() {
        assert_eq!(
            Validation::<u8, _>::invalid_many(vec!["name", "email"]).into_result(),
            Err(vec!["name", "email"])
        );
    }

    #[test]
    fn map_transforms_valid_values() {
        assert_eq!(
            Validation::<_, &str>::valid(1).map(|value| value + 1),
            Validation::valid(2)
        );
    }

    #[test]
    fn map_leaves_errors_untouched() {
        assert_eq!(
            Validation::<u8, _>::invalid("name").map(|value| value + 1),
            Validation::Invalid(vec!["name"])
        );
    }

    #[test]
    fn and_then_chains_valid_values() {
        assert_eq!(
            Validation::<_, &str>::valid(1).and_then(|value| Validation::valid(value + 1)),
            Validation::valid(2)
        );
    }

    #[test]
    fn and_then_short_circuits_invalid_values() {
        assert_eq!(
            Validation::<u8, _>::invalid("name").and_then(|value| Validation::valid(value + 1)),
            Validation::Invalid(vec!["name"])
        );
    }

    #[test]
    fn validity_helpers_report_state() {
        assert!(Validation::<_, &str>::valid(1).is_valid());
        assert!(Validation::<u8, _>::invalid("name").is_invalid());
    }

    #[test]
    fn macro_returns_constructed_value_when_all_bindings_are_valid() {
        let value = crate::validate! {
            name <- Validation::<_, &str>::valid("control"),
            prop <- Validation::valid(42),
            => (name, prop),
        };

        assert_eq!(value.into_result(), Ok(("control", 42)));
    }

    #[test]
    fn macro_returns_one_error_and_skips_constructor() {
        let constructor_called = Cell::new(false);

        let value = crate::validate! {
            name <- Validation::<&str, _>::invalid("name"),
            prop <- Validation::valid(42),
            => {
                constructor_called.set(true);
                (name, prop)
            },
        };

        assert_eq!(value.into_result(), Err(vec!["name"]));
        assert!(!constructor_called.get());
    }

    #[test]
    fn macro_accumulates_multiple_errors_in_order() {
        let value = crate::validate! {
            name <- Validation::<&str, _>::invalid("name"),
            prop <- Validation::<u8, _>::invalid_many(vec!["prop", "format"]),
            => (name, prop),
        };

        assert_eq!(value.into_result(), Err(vec!["name", "prop", "format"]));
    }

    #[test]
    fn macro_preserves_owned_and_borrowed_values() {
        let name = String::from("control");
        let prop = String::from("evidence");

        let value = crate::validate! {
            owned <- Validation::<_, &str>::valid(name),
            borrowed <- Validation::valid(prop.as_str()),
            => (owned, borrowed),
        };

        assert_eq!(
            value.into_result(),
            Ok((String::from("control"), "evidence"))
        );
    }

    #[test]
    fn macro_supports_trailing_comma_before_constructor() {
        let value = crate::validate! {
            name <- Validation::<_, &str>::valid("control"),
            => name
        };

        assert_eq!(value.into_result(), Ok("control"));
    }
}
