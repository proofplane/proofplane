# Trusted Compliance Reads Spec

## Goal

Provide provenance-bearing compliance material for customer agents and
auditor-ready packet views without introducing a submission approval workflow.

## Trust Model

The MVP has two different concepts:

- **Evidence truth:** submissions and attachments are immutable observations.
  File attachments are usable only when `upload_status = uploaded`.
- **Curated material:** a workspace actor creates a concise fact or answer
  fragment and links it to controls, Evidence Requests, and optionally a
  submission. The actor and rationale explain why it is trusted.

Curated material is not automatically generated and does not mark a submission
approved. It may be superseded or retired. Currentness is derived from its own
status plus linked evidence freshness.

## Source Material Model

Add `source_material` with:

- workspace ID, topic, title, body;
- status: `active`, `superseded`, or `retired`;
- author actor, rationale, created/updated timestamps;
- optional superseded-by relationship.

Use mapping tables for controls, Evidence Requests, and Evidence Submissions.
At least one control or Evidence Request link is required. A linked submission
must belong to a linked Evidence Request and the same workspace.

Search supports text query plus control, Evidence Request, status, and freshness
filters. MVP text search uses Postgres full-text search; no vector database or
embedding pipeline is required.

## Freshness

Return:

- `current`: active material with no expired linked evidence;
- `stale`: active material whose newest linked submission is outside the
  Evidence Request freshness window;
- `unusable`: a directly linked submission has attachments but none are
  uploaded, or a referenced attachment is not finalized;
- `retired`: source material status is superseded or retired.

Freshness is evaluated against an injected clock. Missing submissions do not
make an unlinked general control fact stale; they do make material tied to an
Evidence Request stale.

## Auditor Packet Read Model

A packet is generated for one workspace and one or more controls. It contains:

- control and framework-requirement mappings;
- mapped Evidence Requests and schedule/freshness state;
- latest submission metadata and attachment inventory;
- downloadable links only for uploaded attachments;
- linked curated source material with freshness and provenance;
- actor and provenance fields carried by the underlying records;
- generation timestamp and requested-by actor.

The first API returns JSON suitable for product preview. Export produces a ZIP
containing a JSON manifest, a Markdown summary, and uploaded attachment bytes.
Quarantined or failed objects are never included. Packet generation is a read;
the ZIP may stream directly and need not be persisted in the MVP.

## API Contract

```text
POST /workspaces/{workspace_id}/source-material
PUT  /workspaces/{workspace_id}/source-material/{id}
GET  /workspaces/{workspace_id}/source-material/{id}
GET  /workspaces/{workspace_id}/source-material
POST /workspaces/{workspace_id}/auditor-packets/preview
POST /workspaces/{workspace_id}/auditor-packets/export
```

Source-material writes require a new `write_source_material` permission; reads
and packet generation require corresponding read/export permissions.

## Audit Logging

Create/update/retrieve source material and packet preview/export emit structured
`type = "audit_log"` records. Ordinary search results emit one search audit log,
not one record per result. Audit logs are operational Cloud Logging data and are
not embedded in packet responses.

## Revisions

- 2026-06-11: Replaced legacy “approved source material” with explicit actor
  curation because native submission approval is deferred for the MVP.
- 2026-06-11: Replaced database audit events and packet audit-history reads with
  structured application audit logs.
