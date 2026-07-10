# Proofplane MVP Epics

This directory is the source of truth for remaining MVP work. Work is planned
and tracked as epics with lean tickets and one technical spec per epic.

The MVP has two release boundaries:

- **Backend MVP:** an authenticated, auditable compliance backend that accepts
  evidence and exposes trusted compliance reads and writes through the MCP
  agent interface, and runs on production dependencies. _(The REST data-plane
  and `ppat_` API tokens were removed in PR #42 — see the [Agent Connector
  Onboarding](./agent-connector-onboarding/spec.md) 2026-07-09 decision banner.
  MCP is now the sole compliance data interface; REST remains only for
  control-plane routes: auth, `me`, workspaces, OAuth, and browser attachment
  flows.)_
- **Launch MVP:** the backend MVP plus auditor portal access, self-serve
  sandbox, first-run product experience, public pricing, and launch operations.

Native submission approval remains deferred. Trust is explicit: attachments
must be malware-scanned and finalized, submission summaries retain actor
provenance, and freshness is derived from submitted evidence.

## Current Reality

| Area | Implemented | Remaining |
| --- | --- | --- |
| Platform foundation | Rust runtimes, Postgres, config, logging, health routes, Pub/Sub emulator, outbox, worker | Production Pub/Sub startup, dependency hardening, application metrics |
| Identity and authorization | Auth0 users, one workspace per user, workspace membership, MCP OAuth connections (Proofplane PASETO), identity audit logs | `ppat_` API tokens removed in PR #42 |
| Compliance model | Frameworks, controls, Evidence Requests, mappings | Auditor portal reads |
| Evidence lifecycle | Submission create/get/latest, upload integrity, quarantine, ClamAV scan, finalization, download grants, demo seed | Submission context |
| Audit | Structured application logging | Stable audit-log fields and business event coverage; production routing and retention planning is deferred |
| Agent interface | Streamable HTTP MCP runtime and core compliance tools, interactive OAuth authorization (Proofplane facade), working Codex connection | Guided connection UI, Claude/Cowork validation, generic-client support matrix, auditor access link tools |
| Launch surface | Product and GTM notes, PRODUCT.md, DESIGN.md, minimal self-onboarding UI | Guided agent connection and workspace home |

## Epic Portfolio

| Epic | Status | Outcome |
| --- | --- | --- |
| [Auth Hierarchy API](./auth-hierarchy-api/README.md) | Done | Humans self-onboard workspaces and identity actions emit structured audit logs. |
| API Token And PASETO Migration _(archived)_ | Done | Delivered compact workspace-scoped API tokens and encrypted PASETO download grants. Folder removed in commit a36b836; the `ppat_` API-token half was later removed entirely in PR #42, leaving the PASETO/download-grant work. |
| Evidence Lifecycle Completion _(archived)_ | Done | Evidence can be queried, safely downloaded, and demonstrated end to end. Folder removed in commit a36b836. |
| [Production Runtime Adapters](./production-runtime-adapters/README.md) | Todo | GCS and production Google Pub/Sub work without emulator-only assumptions. |
| [Auditor Portal Access](./auditor-portal-access/README.md) | Todo | Auditors review workspace controls, evidence, and eligible attachments through secure browser links. |
| [MCP Server](./mcp-server/README.md) | Doing | MCP is now the sole compliance data-plane. Runtime, read tools, and write tools (001–003) are Done; only logging/equivalence (005) remains, and its REST-parity framing needs reworking since REST was removed. |
| [MCP Attachment Management](./mcp-attachment-upload/README.md) | Todo | Agents hand humans scoped attachment-management links without moving attachment bytes through MCP. |
| [Agent Connector Onboarding](./agent-connector-onboarding/README.md) | Doing | OAuth facade + working Codex connection shipped (tickets 001–004, 007 Done in PR #42). Remaining: guided UI (005), Claude/Cowork (006), generic clients (008). |
| [Reliability and Observability](./reliability-observability/README.md) | Todo | Dependency failures and runtime behavior are visible and tested. |
| [Self-Onboarding UI](./self-onboarding-ui/README.md) | Done - Will Do Later | Remaining UI tickets are postponed until MCP is more complete; specs may need revalidation before reopening. |

## Preferred Sequence

1. Treat API Token And PASETO Migration and Evidence Lifecycle Completion as
   complete foundations.
2. Build the MCP runtime and core evidence/control tools while the shared audit
   contract and evidence audit events land; this produces the core MCP demo.
   Add MCP Attachment Management when the demo needs human attachment management
   without sending bytes through MCP.
3. Build production adapters independently of the core MCP demo milestone.
4. Build Auditor Portal Access after the evidence lifecycle and MCP foundations
   are stable enough to issue links and serve read-only portal data.
5. Add interactive MCP authorization and agent-native distribution through the
   Agent Connector Onboarding epic (the OAuth facade and Codex connection are
   done), then replace the UI's token-centric MCP preview with a verified
   connection flow.
6. Build the remaining Self-Onboarding UI on the stable Auth foundation, using
   preview states where MCP and auditor portal workflows are still in progress.
   Note: the scoped-API-token creation flow is obsolete now that `ppat_` tokens
   are removed (PR #42) — it should be replaced by the OAuth connection flow.
7. Create a separate production-deployment epic when deployment planning begins.

## Definition Of MVP Done

- Every ticket in the backend epics is `Done`, with specs reconciled to what
  shipped.
- `make check` passes, and Docker-backed integration tests cover the release
  flow and external dependency failures.
- Production configuration supports Postgres, Google Pub/Sub, GCS,
  Auth0, and ClamAV without local emulator requirements.
- An OAuth-connected agent can submit evidence, retrieve only finalized
  attachments, inspect summarized evidence, and perform the supported MCP
  workflows. (Previously scoped to user-owned API tokens; `ppat_` was removed
  in PR #42 and MCP OAuth connections are the sole credential.)
- Structured audit logs use the stable identifier-only field contract and cover
  the required business events.
- The launch flow creates a realistic sandbox, connects the customer's agent,
  and produces a useful MCP-backed compliance answer without a sales gate.
