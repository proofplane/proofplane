use crate::domain::WorkspaceId;

pub struct ServiceContext {
    pub workspace_id: WorkspaceId,
}

#[cfg(test)]
mod tests {
    use super::ServiceContext;
    use crate::domain::WorkspaceId;
    use uuid::Uuid;

    #[test]
    fn stores_workspace_context() {
        let workspace_id = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let context = ServiceContext {
            workspace_id: WorkspaceId::from(workspace_id),
        };

        assert_eq!(context.workspace_id, WorkspaceId::from(workspace_id));
    }
}
