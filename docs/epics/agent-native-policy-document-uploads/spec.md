# Agent-Native Policy Document Uploads Spec

## Goal

Allow an authenticated AI agent to upload a policy document that is already
accessible to its trusted runtime without asking a human to open the policy
document management page. File bytes travel directly from that runtime to
Proofplane over HTTP and never appear in MCP arguments, MCP results, or model
context.

The flow extends the agent-native evidence upload contract while preserving
the policy domain rule that an active policy has at most one current,
unarchived document. Proofplane remains the ingestion boundary and continues
to own quarantine storage, checksum calculation, malware scanning, document
finalization, and cleanup.

## Principles And Scope

- Add a policy-specific MCP preparation tool and raw streaming endpoint.
- Keep `manage_policy_document` and its human browser session unchanged.
- Reuse proven machine-transfer primitives without merging evidence
  submissions and policy documents into one domain aggregate.
- Require the same declared file metadata, bounded body, bearer-secret
  handling, idempotency, and cleanup guarantees as agent evidence uploads.
- Create a policy document only when the transfer completes successfully.
- Never archive or replace a current policy document implicitly.
- Keep policy lifecycle polling on the existing `get_policy` tool.

Base64 or multipart content in MCP remains out of scope. Direct-to-provider
presigned uploads are also deferred because they would bypass Proofplane's
current streaming, checksum, quarantine, and completion boundary.

## Domain Model

A **policy document** is the single current file attached directly to an active
policy. It has no evidence submission wrapper or coverage window.

A **human policy document grant** is the existing short-lived authority for a
person to open the browser management experience. That experience may upload,
download, or archive the current document and is unchanged by this epic.

A **machine policy document upload grant** is a short-lived, single-purpose
authority for an agent runtime to transfer one declared file to one policy. It
cannot authorize a download, archive, replacement, evidence submission, or a
second document.

A **policy document upload attempt** is one HTTP transfer made under a machine
policy document upload grant. Failed, interrupted, or concurrent attempts are
not documents. At most one attempt can complete a grant.

Completion means the policy document, scan outbox message, and grant completion
record committed together. The document begins in `pending` and follows the
existing lifecycle through `finalizing` to `uploaded`, `contains_virus`, or
`failed`.

## Existing Baseline And Reuse Boundary

`PolicyDocumentService` already stages streamed chunks in quarantine, computes
length, SHA-256, and CRC32C, creates the policy-owned document and scan outbox
message in one workspace transaction, and cleans staged objects after rejected
creation. The policy repository already locks the active policy and enforces
at most one unarchived policy document.

The agent-native evidence upload flow additionally provides declared-file
validation, a versioned machine credential, raw PUT routing, row-locked
single-winner completion, matching replay, bounded metrics, and failure-path
coverage. These transport and coordination patterns should be shared where
their contracts match.

The implementation must retain distinct domain services, grant aggregates,
persistence, endpoint names, audit events, and metrics for evidence and policy
documents. A generic public upload endpoint or polymorphic grant table is not
part of this epic. Shared internal code must express file-transfer mechanics,
not branch on owner type to decide domain behavior.

## Preparation Contract

Add the MCP tool `prepare_policy_document_upload`.

Input:

```json
{
  "policy_id": "uuid",
  "filename": "information-security-policy.pdf",
  "content_type": "application/pdf",
  "content_length": 483920,
  "checksum_sha256": "optional lowercase hexadecimal digest"
}
```

`content_length` is required, non-negative, and no greater than
`uploads.max_document_bytes`. Existing filename validation applies.
`content_type` must be a non-empty, syntactically valid media type of at most
255 bytes that can be represented in an HTTP `Content-Type` header. The shared
declaration type and both machine-grant tables enforce the same bound. When supplied,
`checksum_sha256` is exactly 64 lowercase hexadecimal characters.

The tool requires `write_controls`. A missing, archived, or cross-workspace
policy receives the existing concealed not-found result. If the active policy
already has a current document, preparation returns a stable policy-document
conflict and persists no grant; the agent can inspect `get_policy`, while any
archive or replacement remains an explicit human management action.

Preparation persists the grant before returning:

```json
{
  "upload_id": "uuid",
  "upload": {
    "method": "PUT",
    "url": "https://api.example/agent-policy-document-uploads/uuid",
    "authorization": "Proofplane-Upload <short-lived-token>",
    "content_type": "application/pdf",
    "expires_at": "RFC 3339 timestamp",
    "max_bytes": 26214400
  }
}
```

No document ID is preallocated. Unlike an evidence submission, a policy
document has no useful domain identity before bytes are accepted, and agents
poll the policy rather than a separately addressable submission. The upload
response supplies the created document ID.

The descriptor is intended for a trusted execution layer. The tool accepts no
local path, attachment handle, or file bytes.

## Machine Grant Persistence

Use a separate `agent_policy_document_upload_grants` table. Do not extend the
existing `policy_document_upload_grants` table: that grant opens a human
browser session, is redeemed before file selection, and does not carry a
declared one-file contract or idempotent completion state.

| Field | Contract |
| --- | --- |
| `id` | Upload ID and primary key. |
| `workspace_id` | Tenant scope. |
| `policy_id` | Target active policy in the same workspace. |
| `filename`, `content_type` | Declared document metadata. |
| `expected_content_length` | Required declared byte count. |
| `expected_sha256` | Optional declared SHA-256 digest. |
| `issued_by_user_id` | User provenance inherited from the connection. |
| `issued_via_agent_connection_id` | Agent connection that prepared the transfer. |
| `issued_at`, `expires_at` | Short-lived validity window. |
| `completed_at`, `document_id` | Null until atomic completion succeeds. |

Persistence enforces filenames and content types of at most 255 bytes,
non-negative length, a valid optional digest, expiry after issuance, valid
completion pairing, a unique completed document ID, and at most one completion
per grant.

Model the grant as a policy-specific aggregate with private state and
read-only accessors. It owns exact credential-authority binding, expiry and
pending eligibility, declared-versus-received matching, rehydration
consistency, and the one-way completion transition. Reuse the evidence grant's
full-snapshot record persistence pattern rather than creating a second style of
grant repository.

### Revision: evidence aggregate and snapshot repository alignment (2026-08-01)

Policy machine upload grants adopt the evidence grant aggregate and persistence
boundary directly. The aggregate owns lifecycle transitions and invariants;
the grant service owns policy eligibility, authorization, provenance, and
completion orchestration. The private persistence record maps the complete
aggregate snapshot, and the repository exposes only workspace-scoped `get` and
transaction-backed `save` operations.

`get` rehydrates the complete workspace-filtered snapshot with `FOR UPDATE`. An autocommit
verification read releases that lock immediately, while a transaction-backed
completion read holds it through commit. `save` performs the shared unscoped
full-snapshot upsert after authorized command orchestration. It does not interpret the operation that
changed the aggregate or query policy and agent-connection relationships;
those checks remain in workspace-scoped services and existing foreign keys.
This revision changes no schema, HTTP, MCP, credential, or domain behavior.

The opaque, versioned credential uses a distinct policy-upload audience and is
bound to the persisted grant ID, workspace, policy, issuing user, issuing agent
connection, and expiry. Possession authorizes only the PUT operation. The
persisted grant remains the source of truth for eligibility and completion.

## Streaming Endpoint

Contract:

```http
PUT /agent-policy-document-uploads/{upload_id}
Authorization: Proofplane-Upload <short-lived-token>
Content-Type: application/pdf
Content-Length: 483920

<streamed file bytes>
```

Before accepting the body, the endpoint verifies the credential, matching path
ID, persisted grant, expiry, workspace and provenance claims, declared headers,
and current policy eligibility. Missing or invalid credentials and unavailable
grants receive one stable unavailable response that does not reveal tenant or
authority details.

The stored filename always comes from the grant. `Content-Type` and
`Content-Length` must match the declaration. Credential and grant authority are
verified before an oversized declaration or stream can be rejected. The route
then applies the configured body limit while streaming, and the ingestion
service independently stops an oversized stream. Received length and optional
SHA-256 are checked against the computed values.

The endpoint streams into a unique quarantine object for each attempt without
buffering the complete body. First completion returns `201 Created` with
`policy_id`, `document_id`, and `upload_status: "pending"`.

## Atomicity, Current-Document Races, And Cleanup

The completion transaction locks the grant and active policy and then:

1. confirms that the grant is still eligible;
2. confirms that the policy has no current unarchived document;
3. creates the policy-owned document with agent user provenance;
4. appends the document-scan outbox message; and
5. records `completed_at` and `document_id` on the grant.

The observable contract is:

- The first valid transfer under a grant commits one document and scan event.
- A matching retry of that completed grant before credential expiry returns
  `200 OK` with the same policy, document, and status.
- A replay whose headers do not match the completed declaration is rejected.
- Concurrent attempts under one grant stage independently, but one commits;
  matching losers delete their object and return the completed result.
- Different machine grants, or machine and browser transfers, may race for a
  policy. Existing repository locking permits one current document; losing
  attempts return a stable current-document conflict and delete their objects.
- A current-document conflict never archives, replaces, or mutates the winner.
  An incomplete losing grant remains usable until expiry only if the current
  document is explicitly archived through the existing management flow.
- Interrupted, mismatched, storage-failed, or rolled-back attempts create no
  document or scan event and remain retryable while the grant is eligible.
- Cleanup failure does not replace the primary response and is logged and
  metered without revealing object keys, metadata, or credentials.

The winning quarantine object is never deleted by a losing attempt. The
existing policy creation invariant remains the final defense against races;
grant issuance checks improve feedback but do not replace completion-time
locking.

## Authorization, Isolation, And Lifecycle

Preparation uses the MCP agent connection and requires `write_controls`. The
grant records its user and agent connection, and completion creates the
document through an agent-connection workspace transaction so existing
provenance remains intact.

The HTTP credential authorizes transfer; it does not recreate an MCP session.
All grant and document reads and writes are workspace-scoped. Stable errors
must not reveal cross-workspace policy, document, grant, user, or connection
identifiers.

After `201` or idempotent `200`, agents call `get_policy(policy_id)` and inspect
the current document's `upload_status`. No new processing status, download
authority, archive tool, or replacement workflow is introduced.

## Audit And Metrics

Stable success audit events are:

- `agent_policy_document_upload_grant.issued`, after preparation commits;
- `agent_policy_document_upload.completed`, after completion commits.

They may include workspace, user, agent connection, request, policy, grant,
and document IDs plus coarse lifecycle status. They never include the
credential, URL, headers, filename, media type, checksum, object key, or bytes.
Rejected, rolled-back, duplicate, and losing attempts do not emit false success
events.

Metrics use bounded result labels:

- `proofplane_agent_policy_document_upload_grants_total{result}` records
  `issued`, `validation_rejected`, `current_document`, `unavailable`, and
  `failed` preparation outcomes.
- `proofplane_agent_policy_document_upload_attempts_total{result}` records
  `created`, `replayed`, `concurrency_lost`, `current_document`,
  `validation_rejected`, `unavailable`, `stream_failed`, `storage_failed`, and
  `database_failed` transfer outcomes.
- `proofplane_agent_policy_document_upload_received_bytes_total` counts bytes
  after a complete stream is staged, including a stream later rejected.
- Existing `proofplane_cleanup_total{operation,result}` records cleanup.

IDs, paths, document metadata, credentials, and error strings are forbidden
metric labels.

## Validation Strategy

Use colocated unit tests for declaration validation, credential claims, grant
state transitions, and pure result mapping. Use Docker-backed integration tests
for persistence, tenant concealment, HTTP streaming, policy locking,
transaction rollback, provenance, outbox creation, object cleanup, and scanner
handoff.

End-to-end coverage includes:

- preparation, transfer, `get_policy` polling, scan, and finalization;
- missing permission and missing, archived, or cross-workspace policy;
- a policy that already has a current document;
- invalid, expired, mismatched, and replayed credentials;
- header, body-limit, actual-length, checksum, and interrupted-stream failures;
- concurrent attempts under one grant and across different grants;
- a machine transfer racing the existing browser transfer;
- storage failure, database rollback, and cleanup failure;
- correct user and agent-connection provenance; and
- unchanged human management, download, archive, and replacement behavior.

Runtime code added or refactored by this epic must not use `.expect(...)`.

## Deferred Work

- Base64 or multipart file content in MCP.
- Direct-to-provider presigned uploads.
- Resumable or multi-part machine transfers.
- Generic public machine-document upload endpoints or polymorphic grant tables.
- Agent-native policy document download, archive, or automatic replacement.
- Generic MCP client attachment-transfer extensions.

## Revisions

- 2026-08-01: Corrected the policy grant persistence boundary to match the
  evidence aggregate and full-snapshot repository pattern; eligibility,
  authorization, and relationship orchestration remain service concerns.
- 2026-08-01: Aligned policy transfers with the corrected evidence authority
  ordering, bounded shared content types to 255 bytes in the domain and both
  machine-grant tables, and recorded issuance immediately after persistence.
- 2026-08-01: Initial spec created from the shipped agent-native evidence
  upload flow. Chose a sibling policy-specific grant and endpoint, reused
  internal transfer machinery, and preserved explicit human-managed archival
  and replacement.
