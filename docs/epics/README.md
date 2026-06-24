# Proofplane MVP Epics

This directory is the source of truth for remaining MVP work. Work is planned
and tracked as epics with lean tickets and one technical spec per epic.

The MVP has two release boundaries:

- **Backend MVP:** an authenticated, auditable compliance backend that accepts
  evidence, exposes trusted compliance reads through REST and MCP, and runs on
  production dependencies.
- **Launch MVP:** the backend MVP plus an auditor-ready packet, self-serve
  sandbox, first-run product experience, public pricing, and launch operations.

Native submission approval remains deferred. Trust is explicit: attachments
must be malware-scanned and finalized, submission summaries retain actor
provenance, and freshness is derived from submitted evidence.

## Current Reality

| Area | Implemented | Remaining |
| --- | --- | --- |
| Platform foundation | Rust runtimes, Postgres, config, logging, health routes, Pub/Sub emulator, outbox, worker | Production Pub/Sub startup, dependency hardening, application metrics |
| Identity and authorization | Auth0 users, workspace membership, user-owned opaque API tokens, actor retirement, identity audit logs |  |
| Compliance model | Frameworks, controls, Evidence Requests, mappings | Submission summaries and auditor packet reads |
| Evidence lifecycle | Submission create/get/latest, upload integrity, quarantine, ClamAV scan, finalization, download grants, demo seed | Submission context |
| Audit | Structured application logging | Stable audit-log fields and business event coverage; production routing and retention planning is deferred |
| Agent interface | MCP binary scaffold | MCP runtime and tools |
| Launch surface | Product and GTM notes, PRODUCT.md, DESIGN.md | Minimal self-onboarding UI and marketing site |

## Epic Portfolio

| Epic | Status | Outcome |
| --- | --- | --- |
| [Auth Hierarchy API](./auth-hierarchy-api/README.md) | Done | Humans self-onboard workspaces and identity actions emit structured audit logs. |
| [API Token And PASETO Migration](./paseto-token-migration/README.md) | Done | Users authenticate with compact workspace-scoped API tokens and download grants use encrypted PASETO. |
| [Evidence Lifecycle Completion](./evidence-lifecycle-completion/README.md) | Done | Evidence can be queried, safely downloaded, and demonstrated end to end. |
| [Production Runtime Adapters](./production-runtime-adapters/README.md) | Todo | GCS and production Google Pub/Sub work without emulator-only assumptions. |
| [Trusted Compliance Reads](./trusted-compliance-reads/README.md) | Todo | Auditor-ready packets expose summarized evidence, provenance, and freshness. |
| [MCP Server](./mcp-server/README.md) | Todo | Agents use the same services and authorization model as REST clients. |
| [Reliability and Observability](./reliability-observability/README.md) | Todo | Dependency failures and runtime behavior are visible and tested. |
| [Self-Onboarding UI](./self-onboarding-ui/README.md) | Todo | A founder can create a workspace, issue a scoped token, and see MCP setup guidance in a realistic SOC 2 sandbox. |

## Preferred Sequence

1. Treat API Token And PASETO Migration and Evidence Lifecycle Completion as
   complete foundations.
2. Build the MCP runtime and core evidence/control tools while the shared audit
   contract and evidence audit events land; this produces the core MCP demo.
3. Build production adapters and the Trusted Compliance Reads packet lane
   independently of that demo milestone.
4. Add MCP auditor-packet tools after preview, export jobs, and download grants
   are stable.
5. Build the Self-Onboarding UI on the stable Auth/API-token foundations, using
   preview states where MCP and packet workflow tickets are still in progress.
6. Create a separate production-deployment epic when deployment planning begins.

## Definition Of MVP Done

- Every ticket in the backend epics is `Done`, with specs reconciled to what
  shipped.
- `make check` passes, and Docker-backed integration tests cover the release
  flow and external dependency failures.
- Production configuration supports Postgres, SpiceDB, Google Pub/Sub, GCS,
  Auth0, and ClamAV without local emulator requirements.
- User-owned API tokens can submit evidence, retrieve only finalized
  attachments, inspect summarized evidence, and perform the supported MCP
  workflows.
- Structured audit logs use the stable identifier-only field contract and cover
  the required business events.
- The launch flow creates a realistic sandbox, connects the customer's agent,
  and produces a useful MCP-backed compliance answer without a sales gate.
