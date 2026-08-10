# Agent-Native Evidence Uploads Spec

## Goal

Allow an authenticated AI agent to submit a file that is already accessible to
its trusted runtime without asking a human to open an upload page. File bytes
must travel directly from that runtime to Proofplane over HTTP; they must never
be encoded in MCP arguments, MCP results, or model context.

Proofplane remains the ingestion boundary. It writes the stream to quarantine,
computes checksums, records the evidence submission and agent provenance, and
queues the existing malware scan and finalization lifecycle.

## Principles And Scope

- Add a machine-specific preparation tool and raw streaming endpoint.
- Keep `manage_evidence_submissions` and its browser upload session unchanged
  for human-managed uploads.
- Reuse the configured `uploads.max_document_bytes`, storage abstraction,
  quarantine layout, checksum calculation, outbox, scanner, and finalizer.
- Allocate the evidence submission ID during preparation so retries converge on
  one result.
- Put only declared metadata and a short-lived transfer descriptor in MCP.
- Treat upload credentials as bearer secrets and exclude them from URLs,
  durable notes, logs, metrics, traces, and audit metadata.

Base64 content in MCP is out of scope because it expands payloads and exposes
evidence to model and protocol logging. Direct-to-cloud presigned upload is
also deferred: it would couple the first version to one storage provider and
move checksum, cleanup, and completion coordination outside Proofplane's
current abstraction.

Portable attachment transfer for generic MCP-only chat clients is not part of
this server epic. Such a client needs a trusted host integration that retains an
opaque attachment handle outside model context and performs the same HTTP
transfer. Coding agents with filesystem and HTTP execution can use the contract
immediately.

## Domain Model

An **evidence submission** remains one evidence file, one coverage window, and
its submitter provenance.

A **human upload grant** authorizes a person to enter the existing browser
experience and may result in multiple submissions. It is not changed by this
epic.

A **machine upload grant** authorizes exactly one declared file and one
preallocated evidence submission. It is scoped to one workspace, evidence
target, coverage window, issuing user, and issuing agent connection. It cannot
authorize reads or another submission.

An **upload attempt** is one HTTP transfer under a machine upload grant. Failed,
interrupted, or concurrent attempts are not submissions. At most one attempt
can complete the grant.

Completion means the evidence submission, document, scan outbox message, and
grant completion record committed together. It does not mean the document is
clean or downloadable; the document begins in `pending` and follows the
existing lifecycle through `finalizing` to `uploaded`, `contains_virus`, or
`failed`.

## Existing Baseline

`EvidenceSubmissionService::upload_document` already streams chunks into
quarantine and calculates length, SHA-256, and CRC32C. Its submission creation
path already creates the evidence submission and document and appends the scan
outbox message in one workspace transaction, deleting the staged object when
creation fails.

The browser route adapts multipart input to that service. The MCP server already
authorizes `write_evidence_submissions`, conceals unavailable evidence when
issuing a human grant, records agent-connection provenance, and exposes
`get_evidence_submission` for lifecycle polling.

The implementation should extract only the transport-neutral behavior needed
by both routes. It must not weaken browser session, download, archive, tenant
isolation, or audit behavior.

## Preparation Contract

Add the MCP tool `prepare_evidence_submission_upload`.

Input:

```json
{
  "evidence_id": "uuid",
  "valid_from": "RFC 3339 timestamp",
  "valid_until": "RFC 3339 timestamp",
  "filename": "access-review.pdf",
  "content_type": "application/pdf",
  "content_length": 483920,
  "checksum_sha256": "optional lowercase hexadecimal digest"
}
```

`content_length` is required, non-negative, and no greater than the configured
maximum. The existing filename validation applies. `content_type` must be a
non-empty, syntactically valid media type that can also be represented in an
HTTP `Content-Type` header. When supplied, `checksum_sha256` is exactly 64
lowercase hexadecimal characters.

The tool requires `write_evidence_submissions`. Missing or cross-workspace
evidence uses the same concealed not-found result as existing submission-write
tools. Evidence does not have an archived state; active, paused, and retired
evidence remain eligible, matching existing submission behavior. Preparation
persists the grant before returning:

```json
{
  "upload_id": "uuid",
  "submission_id": "uuid",
  "upload": {
    "method": "PUT",
    "url": "https://api.example/agent-evidence-uploads/uuid",
    "authorization": "Proofplane-Upload <short-lived-token>",
    "content_type": "application/pdf",
    "expires_at": "RFC 3339 timestamp",
    "max_bytes": 26214400
  }
}
```

The descriptor is intended for a trusted execution layer, not for display to a
human. The tool accepts no local path, attachment handle, or file bytes.

## Machine Upload Grant Persistence

Use a separate `agent_evidence_upload_grants` table so the existing human
grant's browser-session and multi-file semantics do not broaden.

| Field | Contract |
| --- | --- |
| `id` | Upload ID and primary key. |
| `submission_id` | Preallocated, unique submission ID; it is deliberately not foreign-key constrained because the submission is created only at completion. |
| `workspace_id` | Tenant scope. |
| `evidence_id` | Target evidence in the same workspace. |
| `valid_from`, `valid_until` | Coverage window copied into the eventual submission. |
| `filename`, `content_type` | Declared document metadata. |
| `expected_content_length` | Required declared byte count. |
| `expected_sha256` | Optional declared SHA-256 digest. |
| `issued_by_user_id` | User provenance inherited from the connection. |
| `issued_via_agent_connection_id` | Agent connection that prepared the transfer. |
| `issued_at`, `expires_at` | Short-lived validity window. |
| `completed_at`, `document_id` | Null until the atomic completion transaction succeeds. |

Use existing typed IDs and coverage validation, adding a distinct machine-grant
ID type where that prevents accidental use of a human grant. Persistence must
enforce a valid coverage window, expiry after issuance, a unique submission ID,
valid completion pairing, and one completion per grant.

The credential is an opaque, versioned, authenticated token bound to the
persisted grant ID and its workspace, evidence, submission, issuer, and expiry
claims. Possession authorizes only the PUT operation. The persisted row remains
the source of truth for expiry and completion; token validity alone is
insufficient.

### Revision: grant aggregate and persistence boundary (2026-07-31)

The machine upload grant is a domain aggregate rather than a repository record.
It owns exact credential-authority binding, pending and expiry eligibility,
declared and received-file matching, rehydration consistency, and the one-way
completion transition. Its identity, provenance, declaration, timestamps, and
lifecycle state are private and exposed only through read-only accessors.

Persistence uses a private database record and mapper. The concrete grant
repository exposes only `get(upload_id, workspace_id)` and `save(&grant)`.
`get` reads the complete aggregate snapshot with `FOR UPDATE`; verification's
autocommit read releases that lock immediately, while completion reloads through
the transaction-bound repository and holds the lock through commit. `save`
maps and upserts the aggregate's complete current snapshot without interpreting
which domain operation changed it. Existing aggregates are saved only after the
service reloads them under that transaction lock.

A crate-private snapshot helper generates the atomic full-snapshot upsert from
a single private record declaration. Every declared record field is
inserted and every non-conflict field is replaced from the aggregate snapshot;
repositories do not hand-maintain column, parameter, and assignment lists. The
machine grant was the initial consumer; all mutable aggregate repositories now
use the same helper.

Credential decoding produces a typed authority value but makes no authorization
decision. The grant issuance service checks workspace-scoped evidence
eligibility before creating durable state. `AgentEvidenceUploadService`
coordinates authority and aggregate validation, streaming, transaction-bound
reload, submission/document/outbox creation, aggregate completion, and
best-effort cleanup. Lifecycle legality remains in the aggregate; tenant-scoped
queries and domain-to-database mapping remain in persistence. HTTP routes retain
only syntax parsing plus stable response and error mapping. This revision does
not change the schema, endpoint contract, or ticket 004's replay and
losing-attempt semantics.

## Streaming Endpoint

Contract:

```http
PUT /agent-evidence-uploads/{upload_id}
Authorization: Proofplane-Upload <short-lived-token>
Content-Type: application/pdf
Content-Length: 483920

<streamed file bytes>
```

Before accepting the body, the endpoint verifies the token, matching path ID,
persisted grant, expiry, workspace and agent-connection provenance claims, and
declared headers. Missing or invalid credentials and unavailable grants receive
one stable unavailable response that does not reveal whether an ID, workspace,
evidence target, connection, or token claim was wrong.

The stored filename always comes from the grant. `Content-Type` and
`Content-Length` must match the grant. The route applies the configured body
limit before streaming and the ingestion service independently stops a stream
that exceeds it. Length and the optional SHA-256 declaration are compared with
the values computed from the received bytes.

The endpoint streams directly into a unique quarantine object for the attempt.
It does not buffer the complete request in memory. On first completion it
returns `201 Created` with `submission_id`, `document_id`, and initial
`upload_status: "pending"`.

Validation failures use stable, non-sensitive errors. Oversized requests are
rejected as payload too large. Header, actual-length, or checksum mismatches do
not complete the grant and delete any staged object. Storage or database
details are not exposed.

## Atomicity, Retries, And Cleanup

The completion transaction locks the grant and performs these changes as one
unit:

1. confirm that the grant is still eligible;
2. create the evidence submission with the preallocated ID and recorded agent
   provenance;
3. create its document using computed metadata and the winning object key;
4. append the document-scan outbox message; and
5. set `completed_at` and `document_id` on the grant.

The observable contract is:

- The first valid transfer commits exactly one submission and document.
- A retry after success with the same grant credential and matching declared
  metadata returns `200 OK` with the same submission, document, and status.
- A replay whose headers do not match the completed grant is rejected.
- Concurrent attempts may stage independently, but the grant lock permits only
  one commit. Losing objects are deleted and matching losers return the same
  completed metadata.
- An interrupted stream creates no submission and remains retryable until
  expiry. Its partial staged object is deleted.
- Length, checksum, storage, or transaction failure creates no submission or
  scan event, leaves the grant incomplete when retry is safe, and deletes the
  attempt's staged object.
- If cleanup itself fails, the primary response remains stable and the cleanup
  failure is logged and metered without exposing an object key or credential.
- An expired incomplete grant cannot be revived. Completed-result replay still
  requires a valid, unexpired credential.

Object deletion is best-effort only after the durable winner has committed; the
winning quarantine object must never be deleted by a losing attempt. Repository
tests must exercise the real Postgres transaction and row lock.

## Authorization And Isolation

Preparation is authenticated through the MCP agent connection and requires
`write_evidence_submissions`. The grant records both its user and agent
connection; successful submission creation uses the existing agent-connection
workspace transaction so provenance is identical to other agent-originated
evidence writes.

The HTTP transfer is authorized by possession of the machine credential. It
does not impersonate or reauthenticate an MCP session. Instead, the credential
is cryptographically bound to the connection that issued it and checked
against the persisted provenance. This distinction must be explicit in code
and documentation.

Repository reads are workspace-scoped, while saves trust the workspace and
authorization checks performed by command orchestration. Errors must not reveal
cross-workspace evidence, submission, document, grant, user, or connection
identifiers. Logs and metrics must not contain evidence metadata, filenames,
content types, checksums, byte content, tokens, authorization headers, or
internal object keys.

## Lifecycle, Audit, And Metrics

After a `201` or idempotent `200`, agents poll
`get_evidence_submission(submission_id)`. No new processing state machine is
introduced.

Stable structured audit events are:

- `agent_evidence_upload_grant.issued`, after preparation commits;
- `agent_evidence_upload.completed`, after the completion transaction commits.

Events use the existing evidence lifecycle audit helper and may include
workspace, user, agent connection, request, evidence, grant, submission, and
document IDs plus coarse outcome or lifecycle status. They never include the
credential, URL, headers, filename, media type, checksum, object key, or bytes.
Rejected, rolled-back, interrupted, duplicate, and losing attempts must not
emit false success events.

Metrics use the established `proofplane_` prefix and only bounded result
labels. `proofplane_agent_evidence_upload_grants_total{result}` records
`issued`, `validation_rejected`, `unavailable`, and `failed` preparation
outcomes. `proofplane_agent_evidence_upload_attempts_total{result}` records
`created`, `replayed`, `concurrency_lost`, `validation_rejected`, `unavailable`,
`stream_failed`, `storage_failed`, and `database_failed` transfer outcomes.
`proofplane_agent_evidence_upload_received_bytes_total` counts bytes after a
stream is staged, including a complete stream later rejected for metadata
mismatch. Cleanup failures remain visible through
`proofplane_cleanup_total{operation,result}`. IDs, raw paths, metadata,
credentials, and error strings are forbidden labels.

## Validation Strategy

Use colocated unit tests for parsing, validation, token claims, and pure state
decisions. Use Docker-backed integration tests for grant persistence, tenant
concealment, HTTP streaming, transaction rollback, row locking, provenance,
outbox creation, object cleanup, and the scanner handoff.

End-to-end coverage includes:

- successful preparation, transfer, polling, scan, and finalization;
- missing permission and missing or cross-workspace evidence, plus unchanged
  eligibility for active, paused, and retired evidence;
- invalid, expired, mismatched, and replayed credentials;
- missing or mismatched headers, body limit, actual-length mismatch, and
  checksum mismatch;
- interrupted transfer and retry;
- concurrent valid transfers and ambiguous retry after success;
- quarantine storage failure, database rollback, and cleanup failure;
- correct user and agent-connection provenance; and
- unchanged human browser upload behavior.

Runtime code added or refactored by this epic must not use `.expect(...)`.

## Deferred Work

- Base64 or multipart file content in MCP.
- Direct-to-GCS or other provider-specific presigned uploads.
- Generic MCP client attachment-transfer extensions.
- Resumable or multi-part machine transfers.
- Machine uploads for policy documents are promoted to the sibling
  [Agent-Native Policy Document Uploads epic](../agent-native-policy-document-uploads/README.md).
- Download authority derived from a machine upload grant.

## Revisions

- 2026-08-01: Promoted policy document machine uploads from deferred work to a
  sibling epic so the shipped evidence scope remains intact while shared
  transfer mechanics can be reused.
- 2026-08-01: Recorded the shipped machine-upload audit fields, bounded metric
  families, and the repository-standard `proofplane_` metric prefix.
- 2026-07-31: Reconciled evidence eligibility with the shipped domain model.
  Evidence has active, paused, and retired states rather than an archived state,
  and machine grant issuance preserves the existing all-status submission
  behavior.
- 2026-07-29: Initial spec created from the agent-native evidence upload
  handoff. Chose a Proofplane streaming endpoint and a distinct one-file
  machine grant while preserving the human browser flow.
