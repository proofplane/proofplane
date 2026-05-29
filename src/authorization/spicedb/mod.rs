use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;
use tonic::{
    metadata::{errors::InvalidMetadataValue, AsciiMetadataValue},
    transport::{Channel, Endpoint},
    Request, Status,
};
use url::Url;

use crate::{
    config::SpiceDbConfig,
    domain::{ActorContext, WorkspaceId},
};

// The generated rust code for the protos fails these lints. This is generated
// code so we don't care.
#[allow(dead_code, clippy::enum_variant_names, clippy::large_enum_variant)]
mod protos {
    include!(concat!(env!("OUT_DIR"), "/spicedb.rs"));
}

#[derive(Debug, Clone)]
pub struct SpiceDbClient {
    channel: Channel,
    preshared_key: SecretString,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("invalid SpiceDB gRPC endpoint {endpoint}: {source}")]
    Endpoint {
        endpoint: Url,
        #[source]
        source: tonic::transport::Error,
    },
    #[error("failed to connect to SpiceDB at {endpoint}: {source}")]
    Connect {
        endpoint: Url,
        #[source]
        source: tonic::transport::Error,
    },
    #[error("SpiceDB preshared key is not valid gRPC authorization metadata: {0}")]
    AuthorizationMetadata(#[from] InvalidMetadataValue),
    #[error("SpiceDB gRPC request failed: {0}")]
    Request(#[from] Status),
}

impl SpiceDbClient {
    pub async fn from_config(config: &SpiceDbConfig) -> Result<Self, ClientError> {
        let endpoint = Endpoint::from_shared(config.endpoint.to_string()).map_err(|source| {
            ClientError::Endpoint {
                endpoint: config.endpoint.clone(),
                source,
            }
        })?;

        let channel = endpoint
            .connect()
            .await
            .map_err(|source| ClientError::Connect {
                endpoint: config.endpoint.clone(),
                source,
            })?;

        Ok(Self {
            channel,
            preshared_key: config.preshared_key.clone(),
        })
    }

    pub fn preshared_key_is_configured(&self) -> bool {
        !self.preshared_key.expose_secret().is_empty()
    }

    pub async fn write_schema(&self, schema: impl Into<String>) -> Result<(), ClientError> {
        use protos::authzed::api::v1::{
            schema_service_client::SchemaServiceClient, WriteSchemaRequest,
        };

        let mut client = SchemaServiceClient::new(self.channel.clone());
        client
            .write_schema(self.authenticated(WriteSchemaRequest {
                schema: schema.into(),
            })?)
            .await?;

        Ok(())
    }

    pub async fn write_workspace_membership(
        &self,
        workspace_id: WorkspaceId,
        actor_id: &str,
    ) -> Result<(), ClientError> {
        use protos::authzed::api::v1::{
            permissions_service_client::PermissionsServiceClient, WriteRelationshipsRequest,
        };

        let mut client = PermissionsServiceClient::new(self.channel.clone());
        client
            .write_relationships(self.authenticated(WriteRelationshipsRequest {
                updates: vec![workspace_membership_update(workspace_id, actor_id)],
                optional_preconditions: vec![],
                optional_transaction_metadata: None,
            })?)
            .await?;

        Ok(())
    }

    pub async fn check_workspace_permission(
        &self,
        actor: &ActorContext,
        permission: WorkspacePermission,
    ) -> Result<bool, ClientError> {
        use protos::authzed::api::v1::{
            check_permission_response::Permissionship, consistency::Requirement,
            permissions_service_client::PermissionsServiceClient, CheckPermissionRequest,
            Consistency,
        };

        let mut client = PermissionsServiceClient::new(self.channel.clone());
        let response = client
            .check_permission(self.authenticated(CheckPermissionRequest {
                consistency: Some(Consistency {
                    requirement: Some(Requirement::FullyConsistent(true)),
                }),
                resource: Some(object_reference(
                    "workspace",
                    actor.workspace_id.to_string(),
                )),
                permission: permission.as_str().to_owned(),
                subject: Some(actor_subject(&actor.id.to_string())),
                context: None,
                with_tracing: false,
            })?)
            .await?
            .into_inner();

        Ok(response.permissionship == Permissionship::HasPermission as i32)
    }

    fn authenticated<T>(&self, message: T) -> Result<Request<T>, ClientError> {
        let mut request = Request::new(message);
        let token = format!("Bearer {}", self.preshared_key.expose_secret());
        let metadata = AsciiMetadataValue::try_from(token)?;
        request.metadata_mut().insert("authorization", metadata);
        Ok(request)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePermission {
    ReadEvidenceRequests,
    WriteEvidenceRequests,
    ReadControls,
    WriteControls,
}

impl WorkspacePermission {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadEvidenceRequests => "read_evidence_requests",
            Self::WriteEvidenceRequests => "write_evidence_requests",
            Self::ReadControls => "read_controls",
            Self::WriteControls => "write_controls",
        }
    }
}

fn workspace_membership_update(
    workspace_id: WorkspaceId,
    actor_id: &str,
) -> protos::authzed::api::v1::RelationshipUpdate {
    use protos::authzed::api::v1::{relationship_update::Operation, RelationshipUpdate};

    RelationshipUpdate {
        operation: Operation::Touch as i32,
        relationship: Some(workspace_membership(workspace_id, actor_id)),
    }
}

fn workspace_membership(
    workspace_id: WorkspaceId,
    actor_id: &str,
) -> protos::authzed::api::v1::Relationship {
    use protos::authzed::api::v1::Relationship;

    Relationship {
        resource: Some(object_reference("workspace", workspace_id.to_string())),
        relation: "member".to_owned(),
        subject: Some(actor_subject(actor_id)),
        optional_caveat: None,
        optional_expires_at: None,
    }
}

fn actor_subject(actor_id: &str) -> protos::authzed::api::v1::SubjectReference {
    protos::authzed::api::v1::SubjectReference {
        object: Some(object_reference("actor", actor_id.to_owned())),
        optional_relation: String::new(),
    }
}

fn object_reference(
    object_type: &str,
    object_id: String,
) -> protos::authzed::api::v1::ObjectReference {
    protos::authzed::api::v1::ObjectReference {
        object_type: object_type.to_owned(),
        object_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_workspace_membership_as_a_touch_update() {
        use protos::authzed::api::v1::relationship_update::Operation;
        use uuid::Uuid;

        let workspace_id =
            WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap());
        let update =
            workspace_membership_update(workspace_id, "00000000-0000-4000-8000-000000000106");
        let relationship = update.relationship.expect("relationship is present");
        let resource = relationship.resource.expect("resource is present");
        let subject = relationship.subject.expect("subject is present");
        let actor = subject.object.expect("actor is present");

        assert_eq!(update.operation, Operation::Touch as i32);
        assert_eq!(resource.object_type, "workspace");
        assert_eq!(resource.object_id, "00000000-0000-4000-8000-000000000001");
        assert_eq!(relationship.relation, "member");
        assert_eq!(actor.object_type, "actor");
        assert_eq!(actor.object_id, "00000000-0000-4000-8000-000000000106");
        assert!(subject.optional_relation.is_empty());
    }
}
