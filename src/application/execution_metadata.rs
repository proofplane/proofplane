use uuid::Uuid;

/// Transport-neutral identifiers that describe one application execution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecutionMetadata {
    request_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    causation_id: Option<Uuid>,
}

impl ExecutionMetadata {
    pub const fn background() -> Self {
        Self {
            request_id: None,
            correlation_id: None,
            causation_id: None,
        }
    }

    pub const fn for_request(request_id: Uuid) -> Self {
        Self {
            request_id: Some(request_id),
            correlation_id: None,
            causation_id: None,
        }
    }

    pub const fn with_correlation_id(mut self, correlation_id: Uuid) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    pub const fn with_causation_id(mut self, causation_id: Uuid) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    pub const fn request_id(self) -> Option<Uuid> {
        self.request_id
    }

    pub const fn correlation_id(self) -> Option<Uuid> {
        self.correlation_id
    }

    pub const fn causation_id(self) -> Option<Uuid> {
        self.causation_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_keeps_request_correlation_and_causation_distinct() {
        let request_id = Uuid::new_v4();
        let correlation_id = Uuid::new_v4();
        let causation_id = Uuid::new_v4();
        let metadata = ExecutionMetadata::for_request(request_id)
            .with_correlation_id(correlation_id)
            .with_causation_id(causation_id);

        assert_eq!(metadata.request_id(), Some(request_id));
        assert_eq!(metadata.correlation_id(), Some(correlation_id));
        assert_eq!(metadata.causation_id(), Some(causation_id));
    }

    #[test]
    fn background_execution_has_no_transport_identifiers() {
        assert_eq!(
            ExecutionMetadata::background(),
            ExecutionMetadata::default()
        );
    }
}
