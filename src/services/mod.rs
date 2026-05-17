use crate::domain::WorkspaceId;

pub struct ServiceContext {
    pub workspace_id: WorkspaceId,
}

#[cfg(test)]
mod tests {
    use super::ServiceContext;
    use crate::domain::WorkspaceId;

    #[test]
    fn stores_workspace_context() {
        let context = ServiceContext {
            workspace_id: WorkspaceId(42),
        };

        assert_eq!(context.workspace_id, WorkspaceId(42));
    }
}
