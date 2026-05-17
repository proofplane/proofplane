# MVP Plan: Agent-Native Compliance Backend

## Product Thesis

Build a governed compliance backend for AI agents.

The product is not a dashboard-first compliance platform. It is a permissioned compliance state machine exposed through API and MCP. Customers should be able to use their own AI agents to submit evidence, inspect requirements, update mappings, approve submissions, retrieve approved source material, and understand audit history.

The admin UI exists only as a bootstrap, access-control, audit-inspection, and emergency-override surface.

## Core Principles

- Compliance operations happen through agent tools, REST APIs, and MCP.
- The UI should not become a thin Drata clone.
- Evidence mappings should live on durable evidence requirements, not on every recurring evidence submission.
- New evidence submissions should refresh existing requirements without requiring repeated remapping.
- The product should model authority, permissions, provenance, and auditability rather than assuming all review happens through a human UI.
- Agent-submitted or agent-approved changes are acceptable when performed by an actor with the right permissions.

## Core Domain Model

### Evidence Requirements

Evidence requirements are persistent objects representing what evidence is needed.

Examples:

- Quarterly user access review export
- Latest penetration test report
- Current incident response policy
- AWS encryption configuration snapshot

Fields should include:

- title
- description
- expected evidence type
- collection instructions
- owner
- cadence
- due date
- source system
- freshness or expiry rule
- linked controls
- status

Evidence requirements are mapped to controls once. Recurring evidence submissions then satisfy the requirement without needing to be remapped.

### Evidence Submissions

Evidence submissions are artifacts or structured payloads submitted against a requirement.

Examples:

- `access_review_q1_2026.csv`
- `access_review_q2_2026.csv`
- `pentest_report_2026.pdf`

Fields should include:

- requirement ID
- attachment, URL, text, or structured payload
- submitter
- actor type
- collection timestamp
- source system
- collection method
- provenance metadata
- checksum or hash where relevant
- approval status
- replacement or supplement relationship

Submissions inherit the requirement's control mappings by default.

### Controls

Controls are framework-specific objects, initially likely SOC 2 controls.

Control status should be derived from:

- linked evidence requirements
- current approved submissions
- freshness and expiry rules
- missing evidence
- exceptions or emergency overrides

Agents should be able to query control status, identify gaps, and understand which requirements support each control.

### Approved Source Material

Approved source material is the trusted answer bank for downstream questionnaire agents.

It should contain approved facts or answer fragments linked to:

- controls
- evidence requirements
- current approved submissions
- freshness metadata
- approval actor
- approval rationale

The product does not need to generate final questionnaire responses as the main workflow. Customer-owned agents can use this approved material to draft responses elsewhere.

## MVP Features

### 1. Evidence Requirement Management via API/MCP

Agents should be able to:

- create or propose evidence requirements
- list evidence requirements
- get a specific requirement
- update requirement metadata
- inspect collection instructions
- list due or stale requirements
- list requirements mapped to a control
- list controls covered by a requirement

This should not require a normal admin UI workflow.

### 2. Evidence Submission via API/MCP

Agents, humans, integrations, and service accounts should be able to:

- submit evidence for an existing requirement
- upload attachments
- submit structured evidence payloads
- attach provenance metadata
- submit replacement evidence
- check submission status
- query latest approved submission for a requirement

The common workflow is:

1. Agent asks what evidence is due or stale.
2. Backend returns requirements and collection instructions.
3. Agent gathers the artifact or payload.
4. Agent submits it against the existing requirement.
5. Authorized actor approves or rejects the submission.

### 3. Requirement-Level Control Mapping

Mappings should live on evidence requirements.

Agents should be able to:

- map a requirement to a control
- remove a requirement-control mapping
- explain mapping rationale
- list mappings
- approve mapping changes if authorized
- query controls without adequate mapped requirements

Recurring evidence submissions should not need remapping unless the underlying requirement changes.

### 4. Submission Approval

Any authorized actor should be able to approve or reject a submission through API/MCP.

Actors can include:

- human users
- customer-owned AI agents
- service accounts
- integrations
- policy automations

Approval means the submission satisfies the requirement for the relevant period. The linked controls update automatically based on the requirement's mappings and freshness rules.

### 5. Control Registry

Start with one framework, likely SOC 2.

Agents should be able to:

- list controls
- get control status
- list requirements supporting a control
- identify stale or missing evidence
- inspect current approved evidence
- query evidence gaps

### 6. Approved Source Material

Agents should be able to:

- create or propose approved answer material
- retrieve approved source material for a topic or control
- trace answer material back to requirements and submissions
- distinguish approved, stale, expired, blocked, and pending source material

This is the source layer for questionnaire workflows, not the questionnaire drafting layer itself.

### 7. Authorization and Actor Model

Actors should be first-class objects.

Actor types:

- human user
- AI agent
- service account
- integration
- policy automation

Initial permission scopes:

- create evidence requirement
- update evidence requirement
- map requirement to control
- approve requirement mapping
- submit evidence
- approve submission
- reject submission
- read approved evidence
- retrieve approved source material
- update approved source material
- inspect audit history
- administer workspace
- perform emergency override

The product should model authority and trust, not human-versus-AI.

### 8. Agent-Facing REST API

The REST API should cover the same operational surface as MCP.

Read operations:

- list requirements
- get requirement
- list due or stale requirements
- list controls
- get control status
- get control evidence gaps
- search approved evidence
- retrieve approved source material
- inspect audit history

Write operations:

- create or propose requirement
- update requirement
- submit evidence for requirement
- upload attachment
- update submission metadata
- approve submission
- reject submission
- map requirement to control
- approve mapping
- create or update approved source material
- log agent action

### 9. MCP Server

MCP should be a flagship interface, not a side integration.

Initial tools:

- `list_evidence_requirements`
- `get_evidence_requirement`
- `list_due_evidence_requirements`
- `submit_evidence_for_requirement`
- `get_submission_status`
- `get_latest_approved_submission`
- `approve_evidence_submission`
- `reject_evidence_submission`
- `map_requirement_to_control`
- `remove_requirement_control_mapping`
- `list_requirement_control_mappings`
- `get_control_status`
- `get_control_evidence_gaps`
- `find_approved_answer_material`
- `create_or_update_approved_answer_material`
- `inspect_audit_history`
- `log_agent_action`

### 10. Audit Log

All meaningful reads, writes, approvals, mappings, retrievals, and emergency actions should be logged.

Audit events should include:

- actor
- actor type
- timestamp
- action
- object touched
- previous state where relevant
- new state where relevant
- rationale
- source request or session
- API/MCP client identity

The audit log should be queryable through API/MCP and inspectable in the admin UI.

### 11. Bootstrap and Emergency Admin UI

The admin UI should be limited to the minimum needed to bootstrap and control the platform.

Include:

- workspace setup
- company profile basics
- initial framework/template selection
- human user management
- service account management
- AI agent actor registration
- API key or OAuth client issuance and revocation
- MCP connection setup
- credential rotation
- permission and approval-authority configuration
- audit log inspection
- failed API/MCP call inspection
- ingestion/upload failure inspection
- permission-denial inspection
- emergency actor disablement
- emergency credential revocation
- workspace freeze
- emergency removal of approval authority
- emergency blocking of bad evidence or source material
- emergency force-expiry of evidence
- administrative notes and rationale

Do not include normal compliance operations in the admin UI.

Explicitly exclude from the admin UI:

- evidence requirement management
- requirement-to-control mapping
- evidence submission browsing
- approval or rejection review queues
- control coverage views
- freshness or staleness views
- approved source material management
- evidence gap analysis
- normal compliance review workflows
- questionnaire support workflows

Those workflows should happen through API/MCP and be surfaced by the customer's AI agent.

## MVP Demo

1. A setup actor bootstraps a workspace, registers an AI agent, grants permissions, and configures MCP credentials in the minimal admin UI.
2. A customer-owned agent asks the MCP server what evidence is due or stale.
3. The backend returns evidence requirements and collection instructions.
4. The agent gathers the needed artifact for an existing requirement, such as a quarterly access review export.
5. The agent submits the artifact against the existing requirement.
6. An authorized human-supervised agent approves the submission through MCP.
7. The linked control status updates automatically because the requirement was already mapped to controls.
8. A questionnaire agent asks for approved material about access reviews.
9. The backend returns approved facts, linked controls, current approved submissions, and freshness metadata.
10. The audit log records the submission, approval, retrieval, actors, rationale, and session metadata.

## Defer Until Later

Do not build these in the MVP:

- full questionnaire completion workflow
- auditor collaboration portal
- heavy dashboarding
- task management
- vendor management
- risk register
- automated policy generation
- broad framework library
- deep Jira or Slack workflow automation
- complex continuous cloud integrations
- a full human compliance operations UI

## Success Criterion

The MVP should prove that agents can safely operate the compliance evidence lifecycle through a governed API/MCP backend.

Specifically:

- agents can discover stale or due evidence requirements
- agents can submit fresh evidence against existing requirements
- authorized actors can approve submissions
- controls update through durable requirement mappings
- questionnaire agents can retrieve trusted source material
- every important action is permissioned and audited

