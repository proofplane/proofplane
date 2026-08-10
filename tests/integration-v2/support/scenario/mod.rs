pub mod types;

use std::collections::HashMap;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    documents::upload_form,
    harness::TestApp,
    http::{local_path, request_cookie},
    id::workspace_uuid,
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::types::{
        TestControl, TestEvidence, TestEvidenceSubmission, TestFramework, TestPolicy,
        TestPolicyDocument, TestWorkspace,
    },
};
use http::StatusCode;
use proofplane::domain::WorkspacePermission;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use proofplane::routes::request_context::REQUEST_ID_HEADER;
use proofplane::worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED};
use serde_json::json;
use types::TestUser;
use uuid::Uuid;

pub struct ScenarioBuilder<'app> {
    app: &'app TestApp,
    users: Vec<String>,
    workspaces: Vec<String>,
    user_workspaces: HashMap<String, Vec<String>>,
    workspace_evidence: HashMap<String, Vec<String>>,
    workspace_controls: HashMap<String, Vec<(String, String)>>,
    workspace_policies: HashMap<String, Vec<String>>,
    workspace_evidence_documents: HashMap<String, Vec<EvidenceDocumentFixture>>,
    workspace_policy_documents: HashMap<String, Vec<PolicyDocumentFixture>>,
    workspace_evidence_control_mappings: HashMap<String, Vec<EvidenceControlMappingFixture>>,
    workspace_policy_control_mappings: HashMap<String, Vec<PolicyControlMappingFixture>>,
    workspace_control_framework_requirements:
        HashMap<String, Vec<ControlFrameworkRequirementFixture>>,
}

struct EvidenceControlMappingFixture {
    evidence_title: String,
    control_code: String,
    rationale: String,
}

struct PolicyControlMappingFixture {
    policy_name: String,
    control_code: String,
}

struct ControlFrameworkRequirementFixture {
    control_code: String,
    framework_code: String,
    requirement_code: String,
}

struct EvidenceDocumentFixture {
    evidence_title: String,
    filename: String,
    bytes: Vec<u8>,
    valid_from: String,
    valid_until: String,
}

struct PolicyDocumentFixture {
    policy_name: String,
    filename: String,
    bytes: Vec<u8>,
}

impl<'app> ScenarioBuilder<'app> {
    pub fn new(app: &'app TestApp) -> Self {
        Self {
            app,
            users: vec![],
            workspaces: vec![],
            user_workspaces: HashMap::new(),
            workspace_evidence: HashMap::new(),
            workspace_controls: HashMap::new(),
            workspace_policies: HashMap::new(),
            workspace_evidence_documents: HashMap::new(),
            workspace_policy_documents: HashMap::new(),
            workspace_evidence_control_mappings: HashMap::new(),
            workspace_policy_control_mappings: HashMap::new(),
            workspace_control_framework_requirements: HashMap::new(),
        }
    }

    pub fn with_user(mut self, sub: &str) -> Self {
        self.users.push(String::from(sub));

        self
    }

    pub fn with_workspace(mut self, user: &str, name: &str) -> Self {
        let user = String::from(user);
        let name = String::from(name);

        assert!(self.users.contains(&user));

        self.workspaces.push(name.clone());
        self.user_workspaces.entry(user).or_default().push(name);

        self
    }

    pub fn with_evidence(mut self, workspace_name: &str, title: &str) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self.workspaces.contains(&workspace_name));

        self.workspace_evidence
            .entry(workspace_name)
            .or_default()
            .push(String::from(title));

        self
    }

    pub fn with_control(mut self, workspace_name: &str, code: &str, title: &str) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self.workspaces.contains(&workspace_name));

        self.workspace_controls
            .entry(workspace_name)
            .or_default()
            .push((String::from(code), String::from(title)));

        self
    }

    pub fn with_policy(mut self, workspace_name: &str, name: &str) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self.workspaces.contains(&workspace_name));

        self.workspace_policies
            .entry(workspace_name)
            .or_default()
            .push(String::from(name));

        self
    }

    pub fn with_evidence_document(
        mut self,
        workspace_name: &str,
        evidence_title: &str,
        filename: &str,
        bytes: &[u8],
        valid_from: &str,
        valid_until: &str,
    ) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self
            .workspace_evidence
            .get(&workspace_name)
            .is_some_and(|titles| titles.iter().any(|title| title == evidence_title)));
        assert!(!self
            .workspace_evidence_documents
            .get(&workspace_name)
            .is_some_and(|fixtures| fixtures.iter().any(|fixture| {
                fixture.evidence_title == evidence_title && fixture.filename == filename
            })));

        self.workspace_evidence_documents
            .entry(workspace_name)
            .or_default()
            .push(EvidenceDocumentFixture {
                evidence_title: evidence_title.to_owned(),
                filename: filename.to_owned(),
                bytes: bytes.to_vec(),
                valid_from: valid_from.to_owned(),
                valid_until: valid_until.to_owned(),
            });

        self
    }

    pub fn with_policy_document(
        mut self,
        workspace_name: &str,
        policy_name: &str,
        filename: &str,
        bytes: &[u8],
    ) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self
            .workspace_policies
            .get(&workspace_name)
            .is_some_and(|names| names.iter().any(|name| name == policy_name)));
        assert!(!self
            .workspace_policy_documents
            .get(&workspace_name)
            .is_some_and(|fixtures| fixtures
                .iter()
                .any(|fixture| fixture.policy_name == policy_name)));

        self.workspace_policy_documents
            .entry(workspace_name)
            .or_default()
            .push(PolicyDocumentFixture {
                policy_name: policy_name.to_owned(),
                filename: filename.to_owned(),
                bytes: bytes.to_vec(),
            });

        self
    }

    pub fn with_evidence_control_mapping(
        mut self,
        workspace_name: &str,
        evidence_title: &str,
        control_code: &str,
        rationale: &str,
    ) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self.workspaces.contains(&workspace_name));
        assert!(self
            .workspace_evidence
            .get(&workspace_name)
            .is_some_and(|titles| titles.iter().any(|title| title == evidence_title)));
        assert!(self
            .workspace_controls
            .get(&workspace_name)
            .is_some_and(|controls| controls.iter().any(|(code, _)| code == control_code)));

        self.workspace_evidence_control_mappings
            .entry(workspace_name)
            .or_default()
            .push(EvidenceControlMappingFixture {
                evidence_title: String::from(evidence_title),
                control_code: String::from(control_code),
                rationale: String::from(rationale),
            });

        self
    }

    pub fn with_policy_control_mapping(
        mut self,
        workspace_name: &str,
        policy_name: &str,
        control_code: &str,
    ) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self.workspaces.contains(&workspace_name));
        assert!(self
            .workspace_policies
            .get(&workspace_name)
            .is_some_and(|names| names.iter().any(|name| name == policy_name)));
        assert!(self
            .workspace_controls
            .get(&workspace_name)
            .is_some_and(|controls| controls.iter().any(|(code, _)| code == control_code)));

        self.workspace_policy_control_mappings
            .entry(workspace_name)
            .or_default()
            .push(PolicyControlMappingFixture {
                policy_name: String::from(policy_name),
                control_code: String::from(control_code),
            });

        self
    }

    pub fn with_control_framework_requirement(
        mut self,
        workspace_name: &str,
        control_code: &str,
        framework_code: &str,
        requirement_code: &str,
    ) -> Self {
        let workspace_name = String::from(workspace_name);
        assert!(self.workspaces.contains(&workspace_name));
        assert!(self
            .workspace_controls
            .get(&workspace_name)
            .is_some_and(|controls| controls.iter().any(|(code, _)| code == control_code)));

        self.workspace_control_framework_requirements
            .entry(workspace_name)
            .or_default()
            .push(ControlFrameworkRequirementFixture {
                control_code: String::from(control_code),
                framework_code: String::from(framework_code),
                requirement_code: String::from(requirement_code),
            });

        self
    }

    pub async fn build(self) -> Scenario {
        let mut users = Vec::with_capacity(self.users.len());
        let mut workspaces = Vec::with_capacity(self.workspaces.len());

        for sub in self.users {
            let id = self.app.login(&sub).await;

            users.push(TestUser {
                id,
                auth0_sub: sub.clone(),
            });

            for name in self.user_workspaces.get(&sub).unwrap_or(&vec![]) {
                let response = self
                    .app
                    .app_server()
                    .post("/workspace")
                    .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
                    .json(&json!({ "name": name }))
                    .await;
                response.assert_status_ok();

                let workspace_id = workspace_uuid(&response.json());
                let mut evidence =
                    Vec::with_capacity(self.workspace_evidence.get(name).map_or(0, Vec::len));
                let mut controls =
                    Vec::with_capacity(self.workspace_controls.get(name).map_or(0, Vec::len));
                let mut policies =
                    Vec::with_capacity(self.workspace_policies.get(name).map_or(0, Vec::len));

                if let Some(titles) = self.workspace_evidence.get(name) {
                    let token = authorize_agent_connection(
                        self.app,
                        &sub,
                        &format!("Evidence fixtures {workspace_id}"),
                        &[WorkspacePermission::WriteEvidence],
                    )
                    .await;
                    let client = McpClient::connect(self.app.mcp_server(), &token).await;

                    for title in titles {
                        let created = client
                            .call_tool(
                                "create_evidence",
                                json!({
                                    "title": title,
                                    "description": format!("Collect {title}."),
                                    "collection_instructions": format!("Upload {title}."),
                                }),
                            )
                            .await;
                        let id = Uuid::parse_str(
                            created["evidence"]["id"]
                                .as_str()
                                .expect("fixture evidence id is a string"),
                        )
                        .expect("fixture evidence id is a UUID");

                        evidence.push(TestEvidence {
                            id,
                            workspace_id,
                            title: title.clone(),
                            submissions: vec![],
                        });
                    }
                }

                if let Some(fixtures) = self.workspace_evidence_documents.get(name) {
                    let client_name = format!("Evidence document fixtures {workspace_id}");
                    let token = authorize_agent_connection(
                        self.app,
                        &sub,
                        &client_name,
                        &[
                            WorkspacePermission::WriteEvidenceSubmissions,
                            WorkspacePermission::ReadEvidenceSubmissions,
                        ],
                    )
                    .await;
                    let connection_id =
                        get_agent_connection_id_for(self.app, &sub, &client_name).await;
                    let client = McpClient::connect(self.app.mcp_server(), &token).await;

                    for fixture in fixtures {
                        let evidence = evidence
                            .iter_mut()
                            .find(|item| item.title == fixture.evidence_title)
                            .expect("fixture document evidence was created");
                        let grant = client
                            .call_tool(
                                "manage_evidence_submissions",
                                json!({
                                    "evidence_id": evidence.id,
                                    "valid_from": fixture.valid_from,
                                    "valid_until": fixture.valid_until,
                                }),
                            )
                            .await;
                        let redeemed = self
                            .app
                            .app_server()
                            .get(&local_path(
                                grant["url"]
                                    .as_str()
                                    .expect("fixture evidence grant URL is text"),
                            ))
                            .await;
                        redeemed.assert_status(StatusCode::SEE_OTHER);
                        let cookie = request_cookie(
                            redeemed
                                .header("set-cookie")
                                .to_str()
                                .expect("fixture evidence upload cookie is text"),
                        );
                        let mut events = self.app.pipeline_events().subscribe();
                        self.app
                            .app_server()
                            .post("/evidence-document-uploads/files")
                            .add_header("cookie", cookie)
                            .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
                            .multipart(upload_form(&fixture.bytes, &fixture.filename))
                            .await
                            .assert_status(StatusCode::SEE_OTHER);

                        let created = client
                            .call_tool(
                                "list_evidence_submissions",
                                json!({ "evidence_id": evidence.id }),
                            )
                            .await;
                        let created = created["submissions"]
                            .as_array()
                            .expect("fixture evidence submissions is an array")
                            .iter()
                            .find(|submission| {
                                submission["document"]["filename"] == fixture.filename
                            })
                            .expect("fixture evidence submission is listed");
                        let document_id = created["document"]["id"]
                            .as_str()
                            .expect("fixture evidence document id is text");
                        assert_eq!(
                            events
                                .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
                                .await,
                            StatusCode::NO_CONTENT
                        );
                        assert_eq!(
                            events
                                .await_event(DOCUMENT_FINALIZATION_REQUESTED, document_id)
                                .await,
                            StatusCode::NO_CONTENT
                        );

                        let settled = client
                            .call_tool(
                                "list_evidence_submissions",
                                json!({ "evidence_id": evidence.id }),
                            )
                            .await;
                        let settled = settled["submissions"]
                            .as_array()
                            .expect("settled fixture evidence submissions is an array")
                            .iter()
                            .find(|submission| {
                                submission["document"]["filename"] == fixture.filename
                            })
                            .expect("settled fixture evidence submission is listed");
                        let read_model = TestEvidenceSubmission::from_mcp(settled);
                        assert_eq!(read_model.submitted_by_user_id, id);
                        assert_eq!(read_model.submitted_by_agent_connection_id, connection_id);
                        assert_eq!(read_model.upload_status, "uploaded");
                        evidence.submissions.push(read_model);
                    }
                }

                if self.workspace_controls.contains_key(name)
                    || self.workspace_evidence_control_mappings.contains_key(name)
                    || self
                        .workspace_control_framework_requirements
                        .contains_key(name)
                {
                    let token = authorize_agent_connection(
                        self.app,
                        &sub,
                        &format!("Control fixtures {workspace_id}"),
                        &[WorkspacePermission::WriteControls],
                    )
                    .await;
                    let client = McpClient::connect(self.app.mcp_server(), &token).await;

                    if let Some(fixtures) = self.workspace_controls.get(name) {
                        for (code, title) in fixtures {
                            let description = format!("Implement {title}.");
                            let framework_requirement_ids = self
                                .workspace_control_framework_requirements
                                .get(name)
                                .into_iter()
                                .flatten()
                                .filter(|fixture| fixture.control_code == *code)
                                .map(|fixture| {
                                    self.app
                                        .reference_data()
                                        .iter()
                                        .find(|framework| framework.code == fixture.framework_code)
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "suite catalog has no framework with code {}",
                                                fixture.framework_code
                                            )
                                        })
                                        .requirement(&fixture.requirement_code)
                                        .id
                                })
                                .collect::<Vec<_>>();
                            let created = client
                                .call_tool(
                                    "create_control",
                                    json!({
                                        "code": code,
                                        "title": title,
                                        "description": description,
                                        "framework_requirement_ids": framework_requirement_ids,
                                    }),
                                )
                                .await;
                            let id = Uuid::parse_str(
                                created["id"]
                                    .as_str()
                                    .expect("fixture control id is a string"),
                            )
                            .expect("fixture control id is a UUID");

                            controls.push(TestControl {
                                id,
                                workspace_id,
                                code: code.clone(),
                                title: title.clone(),
                                description,
                            });
                        }
                    }

                    if let Some(fixtures) = self.workspace_evidence_control_mappings.get(name) {
                        for fixture in fixtures {
                            let evidence_id = evidence
                                .iter()
                                .find(|item| item.title == fixture.evidence_title)
                                .expect("fixture mapping evidence was created")
                                .id;
                            let control_id = controls
                                .iter()
                                .find(|item| item.code == fixture.control_code)
                                .expect("fixture mapping control was created")
                                .id;

                            client
                                .call_tool(
                                    "map_evidence_to_control",
                                    json!({
                                        "evidence_id": evidence_id,
                                        "control_id": control_id,
                                        "rationale": fixture.rationale,
                                    }),
                                )
                                .await;
                        }
                    }
                }

                if self.workspace_policies.contains_key(name)
                    || self.workspace_policy_documents.contains_key(name)
                    || self.workspace_policy_control_mappings.contains_key(name)
                {
                    let mut permissions = vec![WorkspacePermission::WriteControls];
                    if self.workspace_policy_documents.contains_key(name) {
                        permissions.push(WorkspacePermission::ReadControls);
                    }
                    let token = authorize_agent_connection(
                        self.app,
                        &sub,
                        &format!("Policy fixtures {workspace_id}"),
                        &permissions,
                    )
                    .await;
                    let client = McpClient::connect(self.app.mcp_server(), &token).await;

                    if let Some(names) = self.workspace_policies.get(name) {
                        for name in names {
                            let created = client
                                .call_tool("create_policy", json!({ "name": name }))
                                .await;
                            let id = Uuid::parse_str(
                                created["id"]
                                    .as_str()
                                    .expect("fixture policy id is a string"),
                            )
                            .expect("fixture policy id is a UUID");

                            policies.push(TestPolicy {
                                id,
                                workspace_id,
                                name: name.clone(),
                                description: created["description"].as_str().map(String::from),
                                document: None,
                            });
                        }
                    }

                    if let Some(fixtures) = self.workspace_policy_control_mappings.get(name) {
                        for fixture in fixtures {
                            let policy_id = policies
                                .iter()
                                .find(|item| item.name == fixture.policy_name)
                                .expect("fixture mapping policy was created")
                                .id;
                            let control_id = controls
                                .iter()
                                .find(|item| item.code == fixture.control_code)
                                .expect("fixture mapping control was created")
                                .id;

                            client
                                .call_tool(
                                    "attach_policy_to_control",
                                    json!({
                                        "policy_id": policy_id,
                                        "control_id": control_id,
                                    }),
                                )
                                .await;
                        }
                    }

                    if let Some(fixtures) = self.workspace_policy_documents.get(name) {
                        for fixture in fixtures {
                            let policy = policies
                                .iter_mut()
                                .find(|item| item.name == fixture.policy_name)
                                .expect("fixture document policy was created");
                            let grant = client
                                .call_tool(
                                    "manage_policy_document",
                                    json!({ "policy_id": policy.id }),
                                )
                                .await;
                            let redeemed = self
                                .app
                                .app_server()
                                .get(&local_path(
                                    grant["url"]
                                        .as_str()
                                        .expect("fixture policy grant URL is text"),
                                ))
                                .await;
                            redeemed.assert_status(StatusCode::SEE_OTHER);
                            let cookie = request_cookie(
                                redeemed
                                    .header("set-cookie")
                                    .to_str()
                                    .expect("fixture policy upload cookie is text"),
                            );
                            let mut events = self.app.pipeline_events().subscribe();
                            self.app
                                .app_server()
                                .post("/policy-document-uploads/files")
                                .add_header("cookie", cookie)
                                .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
                                .multipart(upload_form(&fixture.bytes, &fixture.filename))
                                .await
                                .assert_status(StatusCode::SEE_OTHER);

                            let created = client
                                .call_tool("get_policy", json!({ "policy_id": policy.id }))
                                .await;
                            let document_id = created["document"]["id"]
                                .as_str()
                                .expect("fixture policy document id is text");
                            assert_eq!(
                                events
                                    .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
                                    .await,
                                StatusCode::NO_CONTENT
                            );
                            assert_eq!(
                                events
                                    .await_event(DOCUMENT_FINALIZATION_REQUESTED, document_id)
                                    .await,
                                StatusCode::NO_CONTENT
                            );

                            let settled = client
                                .call_tool("get_policy", json!({ "policy_id": policy.id }))
                                .await;
                            let read_model =
                                TestPolicyDocument::from_mcp(policy.id, &settled["document"]);
                            assert_eq!(read_model.filename, fixture.filename);
                            assert_eq!(read_model.upload_status, "uploaded");
                            policy.document = Some(read_model);
                        }
                    }
                }

                workspaces.push(TestWorkspace {
                    id: workspace_id,
                    name: name.clone(),
                    evidence,
                    controls,
                    policies,
                });
            }
        }

        Scenario {
            frameworks: self.app.reference_data().to_vec(),
            users,
            workspaces,
        }
    }
}

pub struct Scenario {
    frameworks: Vec<TestFramework>,
    users: Vec<TestUser>,
    workspaces: Vec<TestWorkspace>,
}

impl Scenario {
    pub fn framework(&self, code: &str) -> &TestFramework {
        self.frameworks
            .iter()
            .find(|framework| framework.code == code)
            .unwrap_or_else(|| panic!("scenario has no framework with code {code}"))
    }

    pub fn user(&self, sub: &str) -> &TestUser {
        self.users
            .iter()
            .find(|user| user.auth0_sub == sub)
            .unwrap_or_else(|| panic!("scenario has a user for {sub}"))
    }

    pub fn workspace(&self, name: &str) -> &TestWorkspace {
        self.workspaces
            .iter()
            .find(|workspace| workspace.name == name)
            .unwrap_or_else(|| panic!("scenario has a workspace named {name}"))
    }
}
