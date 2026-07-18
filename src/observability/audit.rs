//! Transport-neutral structured audit events.
//!
//! Mutation-success events must be emitted only after the service transaction
//! returns successfully. This intentionally leaves a best-effort delivery gap
//! if the process exits after commit but before [`AuditEvent::emit`] runs.

use std::{collections::BTreeMap, fmt};

use uuid::Uuid;

/// The identity responsible for an audited operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditActor {
    User {
        user_id: Uuid,
    },
    AgentConnection {
        user_id: Uuid,
        agent_connection_id: Uuid,
    },
    System {
        name: &'static str,
    },
}

impl AuditActor {
    fn actor_type(self) -> &'static str {
        match self {
            Self::User { .. } => "user",
            Self::AgentConnection { .. } => "agent_connection",
            Self::System { .. } => "system",
        }
    }

    fn user_id(&self) -> Option<&Uuid> {
        match self {
            Self::User { user_id } | Self::AgentConnection { user_id, .. } => Some(user_id),
            Self::System { .. } => None,
        }
    }

    fn agent_connection_id(&self) -> Option<&Uuid> {
        match self {
            Self::AgentConnection {
                agent_connection_id,
                ..
            } => Some(agent_connection_id),
            Self::User { .. } | Self::System { .. } => None,
        }
    }

    fn system_name(self) -> Option<&'static str> {
        match self {
            Self::System { name } => Some(name),
            Self::User { .. } | Self::AgentConnection { .. } => None,
        }
    }
}

/// The client surface through which an audited operation arrived.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditClientType {
    Rest,
    Mcp,
    Worker,
}

impl AuditClientType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Mcp => "mcp",
            Self::Worker => "worker",
        }
    }
}

/// The stable result classification for an audited operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

impl AuditOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Denied => "denied",
        }
    }
}

/// A typed reference to the object affected by an audited operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditObject {
    object_type: &'static str,
    object_id: Uuid,
}

impl AuditObject {
    pub const fn new(object_type: &'static str, object_id: Uuid) -> Self {
        Self {
            object_type,
            object_id,
        }
    }
}

/// A bounded audit record builder containing only UUID identifiers and static names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AuditMetadata(BTreeMap<&'static str, String>);

impl AuditMetadata {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn insert(&mut self, key: &'static str, value: String) {
        self.0.insert(key, value);
    }
}

impl fmt::Display for AuditMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("{}");
        }

        let metadata = serde_json::to_string(&self.0).map_err(|_| fmt::Error)?;
        f.write_str(&metadata)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvent {
    event_id: Uuid,
    event_name: &'static str,
    outcome: AuditOutcome,
    actor: AuditActor,
    client_type: AuditClientType,
    operation: &'static str,
    workspace_id: Option<Uuid>,
    request_id: Option<Uuid>,
    session_id: Option<Uuid>,
    metadata: AuditMetadata,
    object: Option<AuditObject>,
}

impl AuditEvent {
    pub fn new(
        event_name: &'static str,
        outcome: AuditOutcome,
        actor: AuditActor,
        client_type: AuditClientType,
        operation: &'static str,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            event_name,
            outcome,
            actor,
            client_type,
            operation,
            workspace_id: None,
            request_id: None,
            session_id: None,
            metadata: AuditMetadata::default(),
            object: None,
        }
    }

    pub const fn workspace_id(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    pub const fn request_id(mut self, request_id: Uuid) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub const fn session_id(mut self, session_id: Uuid) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn metadata(mut self, key: &'static str, value: impl ToString) -> Self {
        self.metadata.insert(key, value.to_string());
        self
    }

    pub const fn object(mut self, object: AuditObject) -> Self {
        self.object = Some(object);
        self
    }

    /// Emits an `info` event with an explicit root parent.
    pub fn emit(self) {
        let object_type = self.object.map(|object| object.object_type);
        let object_id = self.object.as_ref().map(|object| object.object_id);

        tracing::info!(
            target: "proofplane::audit",
            parent: None,
            r#type = "audit_log",
            event_id = %self.event_id,
            event_name = self.event_name,
            outcome = self.outcome.as_str(),
            actor_type = self.actor.actor_type(),
            user_id = self.actor.user_id().map(|id| id.to_string()),
            agent_connection_id = self.actor.agent_connection_id().map(|id| id.to_string()),
            system_name = self.actor.system_name(),
            client_type = self.client_type.as_str(),
            operation = self.operation,
            workspace_id = self.workspace_id.map(|id| id.to_string()),
            request_id = self.request_id.map(|id| id.to_string()),
            session_id = self.session_id.map(|id| id.to_string()),
            metadata = %self.metadata,
            object_type,
            object_id = object_id.map(|id| id.to_string()),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use chrono::DateTime;
    use serde_json::Value;
    use tracing::subscriber::with_default;
    use tracing_subscriber::fmt::MakeWriter;
    use uuid::Uuid;

    use super::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome};

    #[test]
    fn emits_required_and_optional_fields_as_json() {
        let user_id = Uuid::new_v4();
        let agent_connection_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let evidence_request_id = Uuid::new_v4();
        let evidence_submission_id = Uuid::new_v4();
        let evidence_document_id = Uuid::new_v4();
        let object_id = Uuid::new_v4();

        let record = capture(|| {
            AuditEvent::new(
                "evidence_submission.created",
                AuditOutcome::Success,
                AuditActor::AgentConnection {
                    user_id,
                    agent_connection_id,
                },
                AuditClientType::Mcp,
                "create_evidence_submission",
            )
            .workspace_id(workspace_id)
            .request_id(request_id)
            .session_id(session_id)
            .metadata("evidence_request_id", evidence_request_id)
            .metadata("evidence_submission_id", evidence_submission_id)
            .metadata("evidence_document_id", evidence_document_id)
            .metadata("lifecycle_status", "uploaded")
            .object(AuditObject::new("evidence_submission", object_id))
            .emit();
        });
        let fields = &record["fields"];

        assert_eq!(record["level"], "INFO");
        assert!(DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap()).is_ok());
        assert_eq!(fields["type"], "audit_log");
        assert!(Uuid::parse_str(fields["event_id"].as_str().unwrap()).is_ok());
        assert_eq!(fields["event_name"], "evidence_submission.created");
        assert_eq!(fields["outcome"], "success");
        assert_eq!(fields["actor_type"], "agent_connection");
        assert_eq!(fields["user_id"], user_id.to_string());
        assert_eq!(
            fields["agent_connection_id"],
            agent_connection_id.to_string()
        );
        assert_eq!(fields["client_type"], "mcp");
        assert_eq!(fields["operation"], "create_evidence_submission");
        assert_eq!(fields["workspace_id"], workspace_id.to_string());
        assert_eq!(fields["request_id"], request_id.to_string());
        assert_eq!(fields["session_id"], session_id.to_string());
        assert_eq!(
            fields["metadata"],
            format!(
                r#"{{"evidence_document_id":"{}","evidence_request_id":"{}","evidence_submission_id":"{}","lifecycle_status":"uploaded"}}"#,
                evidence_document_id, evidence_request_id, evidence_submission_id
            )
        );
        let metadata: serde_json::Value =
            serde_json::from_str(fields["metadata"].as_str().unwrap()).unwrap();
        assert_eq!(
            metadata["evidence_request_id"],
            evidence_request_id.to_string()
        );
        assert_eq!(
            metadata["evidence_submission_id"],
            evidence_submission_id.to_string()
        );
        assert_eq!(
            metadata["evidence_document_id"],
            evidence_document_id.to_string()
        );
        assert_eq!(metadata["lifecycle_status"], "uploaded");
        assert!(fields.get("evidence_request_id").is_none());
        assert!(fields.get("evidence_submission_id").is_none());
        assert!(fields.get("evidence_document_id").is_none());
        assert!(fields.get("lifecycle_status").is_none());
        assert_eq!(fields["object_type"], "evidence_submission");
        assert_eq!(fields["object_id"], object_id.to_string());
        assert!(fields.get("system_name").is_none());
    }

    #[test]
    fn omits_unsupplied_optional_fields_and_keeps_enum_values_stable() {
        let record = capture(|| {
            AuditEvent::new(
                "worker.delivery",
                AuditOutcome::Denied,
                AuditActor::System { name: "worker" },
                AuditClientType::Worker,
                "handle_delivery",
            )
            .emit();
        });
        let fields = &record["fields"];

        assert_eq!(fields["outcome"], "denied");
        assert_eq!(fields["actor_type"], "system");
        assert_eq!(fields["system_name"], "worker");
        assert_eq!(fields["client_type"], "worker");
        for field in [
            "user_id",
            "agent_connection_id",
            "workspace_id",
            "request_id",
            "session_id",
            "object_type",
            "object_id",
        ] {
            assert!(fields.get(field).is_none(), "unexpected field {field}");
        }

        assert_eq!(AuditOutcome::Failure.as_str(), "failure");
        assert_eq!(AuditClientType::Mcp.as_str(), "mcp");
    }

    #[test]
    fn explicit_root_parent_prevents_span_data_from_leaking() {
        let record = capture(|| {
            let span = tracing::info_span!(
                "unsafe_context",
                credential = "credential-sentinel",
                url = "url-sentinel",
                object_key = "object-key-sentinel",
                bytes = "bytes-sentinel",
                description = "description-sentinel",
                error = "error-sentinel",
            );
            let _guard = span.enter();

            AuditEvent::new(
                "user.login",
                AuditOutcome::Success,
                AuditActor::User {
                    user_id: Uuid::new_v4(),
                },
                AuditClientType::Rest,
                "login",
            )
            .emit();
        });
        let serialized = serde_json::to_string(&record).unwrap();

        for sentinel in [
            "credential",
            "credential-sentinel",
            "url",
            "url-sentinel",
            "object_key",
            "object-key-sentinel",
            "bytes",
            "bytes-sentinel",
            "description",
            "description-sentinel",
            "error",
            "error-sentinel",
        ] {
            assert!(
                !serialized.contains(sentinel),
                "leaked {sentinel}: {serialized}"
            );
        }
        assert!(record.get("span").is_none());
        assert!(record.get("spans").is_none());
    }

    fn capture(emit: impl FnOnce()) -> Value {
        let output = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_file(false)
            .with_line_number(false)
            .with_writer(output.clone())
            .finish();

        with_default(subscriber, emit);

        let bytes = output.0.lock().unwrap().clone();
        serde_json::from_slice(&bytes).expect("audit event is JSON")
    }

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
