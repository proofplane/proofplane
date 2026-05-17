pub fn default_log_filter() -> &'static str {
    "info"
}

#[cfg(test)]
mod tests {
    use super::default_log_filter;

    #[test]
    fn has_default_log_filter() {
        assert_eq!(default_log_filter(), "info");
    }
}
