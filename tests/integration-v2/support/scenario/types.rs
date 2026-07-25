use chrono::{DateTime, FixedOffset};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone)]
pub struct TestFramework {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: String,
    pub(in crate::support) requirements: Vec<TestFrameworkRequirement>,
}

#[derive(Clone)]
pub struct TestFrameworkRequirement {
    pub id: Uuid,
    pub framework_id: Uuid,
    pub code: String,
    pub title: String,
    pub description: String,
}

pub struct TestUser {
    pub id: Uuid,
    pub auth0_sub: String,
}

pub struct TestWorkspace {
    pub id: Uuid,
    pub name: String,
    pub(super) evidence: Vec<TestEvidence>,
    pub(super) controls: Vec<TestControl>,
    pub(super) policies: Vec<TestPolicy>,
}

pub struct TestEvidence {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub(super) submissions: Vec<TestEvidenceSubmission>,
}

pub struct TestEvidenceSubmission {
    pub id: Uuid,
    pub evidence_id: Uuid,
    pub valid_from: DateTime<FixedOffset>,
    pub valid_until: DateTime<FixedOffset>,
    pub received_at: DateTime<FixedOffset>,
    pub submitted_by_user_id: Uuid,
    pub submitted_by_agent_connection_id: Uuid,
    pub filename: String,
    pub document_id: Uuid,
    pub upload_status: String,
}

pub struct TestControl {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub code: String,
    pub title: String,
    pub description: String,
}

pub struct TestPolicy {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub(super) document: Option<TestPolicyDocument>,
}

pub struct TestPolicyDocument {
    pub policy_id: Uuid,
    pub document_id: Uuid,
    pub filename: String,
    pub created_at: DateTime<FixedOffset>,
    pub upload_status: String,
}

impl TestFramework {
    pub fn requirement(&self, code: &str) -> &TestFrameworkRequirement {
        self.requirements
            .iter()
            .find(|requirement| requirement.framework_id == self.id && requirement.code == code)
            .unwrap_or_else(|| {
                panic!(
                    "scenario framework {} has no requirement with code {code}",
                    self.code
                )
            })
    }
}

impl TestWorkspace {
    pub fn evidence(&self, title: &str) -> &TestEvidence {
        self.evidence
            .iter()
            .find(|evidence| evidence.workspace_id == self.id && evidence.title == title)
            .unwrap_or_else(|| {
                panic!(
                    "scenario workspace {} has no evidence titled {title}",
                    self.name
                )
            })
    }

    pub fn control(&self, code: &str) -> &TestControl {
        self.controls
            .iter()
            .find(|control| control.workspace_id == self.id && control.code == code)
            .unwrap_or_else(|| {
                panic!(
                    "scenario workspace {} has no control with code {code}",
                    self.name
                )
            })
    }

    pub fn policy(&self, name: &str) -> &TestPolicy {
        self.policies
            .iter()
            .find(|policy| policy.workspace_id == self.id && policy.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "scenario workspace {} has no policy named {name}",
                    self.name
                )
            })
    }
}

impl TestEvidence {
    pub fn submission(&self, filename: &str) -> &TestEvidenceSubmission {
        self.submissions
            .iter()
            .find(|submission| submission.evidence_id == self.id && submission.filename == filename)
            .unwrap_or_else(|| {
                panic!(
                    "scenario evidence {} has no submission named {filename}",
                    self.title
                )
            })
    }
}

impl TestEvidenceSubmission {
    pub(crate) fn from_mcp(projection: &Value) -> Self {
        let submission = &projection["submission"];
        let document = &projection["document"];

        Self {
            id: parse_uuid(&submission["id"], "evidence submission id"),
            evidence_id: parse_uuid(&submission["evidence_id"], "submission evidence id"),
            valid_from: parse_timestamp(&submission["valid_from"], "submission valid_from"),
            valid_until: parse_timestamp(&submission["valid_until"], "submission valid_until"),
            received_at: parse_timestamp(&submission["received_at"], "submission received_at"),
            submitted_by_user_id: parse_uuid(
                &submission["submitted_by"]["user_id"],
                "submission user id",
            ),
            submitted_by_agent_connection_id: parse_uuid(
                &submission["submitted_by"]["agent_connection_id"],
                "submission agent connection id",
            ),
            filename: document["filename"]
                .as_str()
                .expect("submission document filename is text")
                .to_owned(),
            document_id: parse_uuid(&document["id"], "evidence document id"),
            upload_status: document["upload_status"]
                .as_str()
                .expect("evidence document upload status is text")
                .to_owned(),
        }
    }
}

impl TestPolicy {
    pub fn document(&self) -> &TestPolicyDocument {
        self.document
            .as_ref()
            .unwrap_or_else(|| panic!("scenario policy {} has no document", self.name))
    }
}

impl TestPolicyDocument {
    pub(crate) fn from_mcp(policy_id: Uuid, projection: &Value) -> Self {
        Self {
            policy_id,
            document_id: parse_uuid(&projection["id"], "policy document id"),
            filename: projection["filename"]
                .as_str()
                .expect("policy document filename is text")
                .to_owned(),
            created_at: parse_timestamp(&projection["created_at"], "policy document created_at"),
            upload_status: projection["upload_status"]
                .as_str()
                .expect("policy document upload status is text")
                .to_owned(),
        }
    }
}

fn parse_uuid(value: &Value, label: &str) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap_or_else(|| panic!("{label} is text")))
        .unwrap_or_else(|error| panic!("{label} is a UUID: {error}"))
}

fn parse_timestamp(value: &Value, label: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(value.as_str().unwrap_or_else(|| panic!("{label} is text")))
        .unwrap_or_else(|error| panic!("{label} is RFC 3339: {error}"))
}
