pub const PROOFPLANE_ENV: &str = "PROOFPLANE_ENV";

#[cfg(test)]
mod tests {
    use super::PROOFPLANE_ENV;

    #[test]
    fn defines_environment_variable_name() {
        assert_eq!(PROOFPLANE_ENV, "PROOFPLANE_ENV");
    }
}
