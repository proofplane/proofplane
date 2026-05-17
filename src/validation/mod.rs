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

    pub fn into_result(self) -> Result<T, Vec<E>> {
        match self {
            Self::Valid(value) => Ok(value),
            Self::Invalid(errors) => Err(errors),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Validation;

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
}
