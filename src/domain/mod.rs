#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceId(pub u64);

#[cfg(test)]
mod tests {
    use super::WorkspaceId;

    #[test]
    fn workspace_id_is_value_type() {
        assert_eq!(WorkspaceId(1), WorkspaceId(1));
    }
}
