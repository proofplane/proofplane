pub trait HealthRepository {
    fn is_ready(&self) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct InMemoryHealthRepository;

impl HealthRepository for InMemoryHealthRepository {
    fn is_ready(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{HealthRepository, InMemoryHealthRepository};

    #[test]
    fn in_memory_repository_is_ready() {
        let repository = InMemoryHealthRepository;

        assert!(repository.is_ready());
    }
}
