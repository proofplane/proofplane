pub fn startup_message() -> &'static str {
    "proofplane api scaffold ready"
}

#[cfg(test)]
mod tests {
    use super::startup_message;

    #[test]
    fn has_startup_message() {
        assert_eq!(startup_message(), "proofplane api scaffold ready");
    }
}
