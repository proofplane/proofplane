#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicName(String);

impl TopicName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::TopicName;

    #[test]
    fn stores_topic_name() {
        let topic = TopicName::new("events");

        assert_eq!(topic.as_str(), "events");
    }
}
