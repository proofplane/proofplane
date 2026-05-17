#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectKey(String);

impl ObjectKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectKey;

    #[test]
    fn stores_object_key() {
        let key = ObjectKey::new("workspace/evidence.txt");

        assert_eq!(key.as_str(), "workspace/evidence.txt");
    }
}
