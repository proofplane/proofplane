# Trusted Compliance Reads Spec

## Goal

Provide provenance-bearing, auditor-ready packet views without introducing a
submission approval workflow or a second curated-content model.

## Trust Model

The MVP treats submissions and attachments as immutable observations. An
optional immutable Evidence Submission summary states what the evidence
demonstrates and carries the submission's existing actor and collection
provenance. File attachments are usable only when `upload_status = uploaded`.
Neither a summary nor an uploaded attachment marks a submission approved.

## Evidence Freshness

For each mapped Evidence Request, packet assembly returns `current`, `stale`,
`missing`, or `unusable`. Currentness is derived from the request freshness
window and latest submission. Missing means no submission exists. Unusable
means the selected submission has attachments but none are uploaded, or a
referenced attachment is not finalized.

Freshness is evaluated against an injected clock so repeated evaluation is
deterministic.

## Auditor Packet Read Model

A packet is generated for one workspace and one or more controls. Its compact
preview contains:

- control and framework-requirement mappings;
- mapped Evidence Requests and schedule/freshness state;
- latest submission metadata and attachment inventory;
- grant-creation actions only for uploaded attachments;
- actor and provenance fields carried by the underlying records;
- generation timestamp and requested-by actor.

The preview omits submission summaries and descriptions to keep aggregate REST
and MCP results bounded. It also omits persistent or pre-generated download
URLs; a caller requests a fresh grant for an eligible attachment when a human
chooses to inspect it.

Export is asynchronous. A request creates a durable export record and an outbox
message in one transaction. The worker assembles a ZIP containing a JSON
manifest, a Markdown summary, bounded submission summaries, and uploaded
attachment bytes, then writes it to object storage. It does not include
submission descriptions. Quarantined or failed objects are never included.

## Packet Export Model

Persist an export ID, workspace, requesting user and API token, selected control
IDs, status, timestamps, stable failure code, and nullable object metadata. The
object key is persisted for internal retrieval but never serialized externally.
Selection is immutable after creation. The worker evaluates the current packet
read model when it starts building rather than treating an earlier preview as a
snapshot; the ZIP manifest records its own `generated_at` timestamp.

An export moves through `pending`, `building`, `ready`, or `failed`. The worker
uses the export ID as the idempotency key and object-name component, verifies
attachment eligibility and object metadata while building, and marks the row
`ready` only after the ZIP object and its length and SHA-256 metadata are
durable. Retryable failures leave the job eligible for redelivery; exhausted or
non-retryable failures mark it `failed` without exposing dependency details.
ZIP assembly must use bounded memory and disk rather than buffering the packet
or all attachments in memory.

Ready exports remain available for 24 hours after `completed_at`; the worker
sets `expires_at` when it marks the export ready. The application rejects grants
and downloads after `expires_at`; object-storage lifecycle policy removes
expired objects under the dedicated packet-export prefix. Filesystem-backed
local development may clean them up best-effort. Export object keys are
internal and never appear in API, MCP, audit, or grant responses.

## Packet Download Grants

A ready export can receive a stateless, five-minute encrypted download grant.
The URL is intended for a human browser and follows the attachment-download
contract: it is reusable until expiry, is a bearer secret, and streams through
Proofplane with `Cache-Control: private, no-store`, `Referrer-Policy:
no-referrer`, and a safe ZIP `Content-Disposition` filename. Grant issuance
returns `409 packet_not_ready` while work is pending/building and never issues a
grant for failed or expired exports.

The grant uses a packet-specific audience and carries version, export,
workspace, requesting user, and API-token identifiers. Every download verifies
the token, reloads the export, requires `ready` and unexpired state, and checks
stored object metadata before streaming. The worker and download path use the
shared runtime object-store abstraction: filesystem locally and GCS in
production. ZIP bytes never pass through MCP or model context.

## API Contract

```text
POST /workspaces/{workspace_id}/auditor-packets/preview
POST /workspaces/{workspace_id}/auditor-packet-exports
GET  /workspaces/{workspace_id}/auditor-packet-exports/{export_id}
POST /workspaces/{workspace_id}/auditor-packet-exports/{export_id}/download-grants
GET  /auditor-packet-downloads?token=<PASETO>
```

The create endpoint validates the control selection and returns `202` with the
export ID, status, and creation time. The status endpoint returns only compact
lifecycle and result metadata, including `retry_after_seconds` while work is
pending/building and `expires_at` once ready. It never returns ZIP bytes, object
keys, attachment contents, or a download URL. Packet generation and grant
issuance require the packet export permission.

## Audit Logging

Packet preview, export request/completion/failure, grant issuance, and download
redemption emit structured `type = "audit_log"` records. Audit logs are
operational Cloud Logging data and are not embedded in packet responses. They
never contain grant tokens, URLs, object keys, ZIP bytes, attachment bytes, or
submission free text.

## Deferred Source Material

A standalone curated source-material record with independent authorship,
lifecycle, cross-record mappings, and full-text search is deferred. It should be
reconsidered only when a demonstrated workflow requires narratives spanning
multiple controls, Evidence Requests, and Evidence Submissions. Search over
submission summaries and existing metadata should be evaluated first.

## Revisions

- 2026-06-11: Replaced legacy “approved source material” with explicit actor
  curation because native submission approval is deferred for the MVP.
- 2026-06-11: Replaced database audit events and packet audit-history reads with
  structured application audit logs.
- 2026-06-20: Replaced the planned standalone source-material MVP with optional
  immutable Evidence Submission context; deferred cross-record curation and
  search until a concrete workflow requires them. Packet previews omit both
  context fields; exports include only the bounded summary.
- 2026-06-20: Replaced synchronous ZIP streaming with an outbox-backed worker
  export, persisted object, compact status polling, and a short-lived
  human-download grant so agents never mediate packet bytes.
