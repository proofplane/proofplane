# Proofplane MVP Epics

This directory is the source of truth for remaining MVP work. The legacy
[MVP stories](../mvp-stories/README.md) record the original build sequence, but
new work is planned and tracked as epics with lean tickets and one technical
spec per epic.

The MVP has two release boundaries:

- **Backend MVP:** an authenticated, auditable compliance backend that accepts
  evidence, exposes trusted compliance reads through REST and MCP, and runs on
  production dependencies.
- **Launch MVP:** the backend MVP plus an auditor-ready packet, self-serve
  sandbox, first-run product experience, public pricing, and launch operations.

Native submission approval remains deferred. Trust is explicit: attachments
must be malware-scanned and finalized, source material is curated with
provenance, and freshness is derived from linked evidence.

## Current Reality

| Area | Implemented | Remaining |
| --- | --- | --- |
| Platform foundation | Rust runtimes, Postgres, config, logging, health routes, Pub/Sub emulator, outbox, worker | Production Pub/Sub startup, dependency hardening, application metrics |
| Identity and authorization | API-key data plane, Auth0 users, workspace self-onboarding and membership | Workspace actors, key rotation, identity audit logs |
| Compliance model | Frameworks, controls, Evidence Requests, mappings | Trusted source material and auditor packet reads |
| Evidence lifecycle | Submission create/get, upload integrity, quarantine, ClamAV scan, finalization | Latest API, download enforcement, demo submission/object seed |
| Audit | Structured application logging | Stable audit-log fields, Cloud Logging retention, business event coverage |
| Agent interface | MCP binary scaffold | MCP runtime and tools |
| Launch surface | Product and GTM notes | Minimal sandbox/MCP setup UI and marketing site |

## Epic Portfolio

| Epic | Status | Legacy source | Outcome |
| --- | --- | --- | --- |
| [Auth Hierarchy API](./auth-hierarchy-api/README.md) | Doing | Extends 010 | Humans manage workspace actors and rotating API keys. |
| [Evidence Lifecycle Completion](./evidence-lifecycle-completion/README.md) | Todo | 017 | Evidence can be queried, safely downloaded, and demonstrated end to end. |
| [Production Runtime Adapters](./production-runtime-adapters/README.md) | Todo | 011, 014 | GCS and production Google Pub/Sub work without emulator-only assumptions. |
| [Trusted Compliance Reads](./trusted-compliance-reads/README.md) | Todo | 018, 019, 025 | Curated source material and auditor-ready packets expose provenance and freshness. |
| [MCP Server](./mcp-server/README.md) | Todo | 021 | Agents use the same services and authorization model as REST clients. |
| [Reliability and Observability](./reliability-observability/README.md) | Todo | 022, 023 | Dependency failures and runtime behavior are visible and tested. |
| [Sandbox Product Launch](./sandbox-product-launch/README.md) | Todo | 025 | A founder can connect an agent to a realistic SOC 2 sandbox. |

## Preferred Sequence

1. Finish `auth-hierarchy-api/003` and the Evidence Lifecycle Completion epic.
2. Establish the structured audit-log contract in Reliability and
   Observability; identity and data-plane emission can then proceed in parallel.
3. Build production adapters while Trusted Compliance Reads starts on the
   completed evidence model.
4. Build MCP after actor management and trusted-read contracts are stable.
5. Add reliability coverage and metrics continuously alongside product work.
6. Build the Sandbox Product Launch on the stable APIs and packet preview.
7. Create a separate production-deployment epic when deployment planning begins.

## Legacy Story Crosswalk

| Stories | Reconciliation |
| --- | --- |
| 001-009 | Implemented foundation; no new epic required. |
| 010 | Original API-key auth is implemented; customer actor/key management continues in Auth Hierarchy API. |
| 011-013 | Local Pub/Sub, outbox, dequeuer, and worker are implemented; production Pub/Sub moves to Production Runtime Adapters. |
| 014 | Filesystem storage is implemented; GCS moves to Production Runtime Adapters. |
| 015-016 | Evidence Requests, controls, and mappings are implemented. |
| 017 | Scan/finalization is implemented; remaining work moves to Evidence Lifecycle Completion. |
| 018 | Native approval remains deferred; usability derives from attachment status and freshness. |
| 019 | Reframed as curated Trusted Compliance Reads without a submission approval dependency. |
| 020 | Reframed as structured audit logging owned by Reliability and Observability plus each instrumented domain epic; no audit table or query API. |
| 021 | Moves to MCP Server with a reduced, implementable MVP tool set. |
| 022-023 | Move to Reliability and Observability. Existing worker rollback tests are baseline, not remaining scope. |
| 024 | Deferred. A production-deployment epic will be created from current infrastructure decisions when deployment work begins. |
| 025 | Splits backend packet reads from the UI/marketing work in Sandbox Product Launch. |

## Definition Of MVP Done

- Every ticket in the backend epics is `Done`, with specs reconciled to what
  shipped.
- `make check` passes, and Docker-backed integration tests cover the release
  flow and external dependency failures.
- Production configuration supports Postgres, SpiceDB, Google Pub/Sub, GCS,
  Auth0, and ClamAV without local emulator requirements.
- Actors can submit evidence, retrieve only finalized attachments, query trusted
  compliance material, and perform the supported MCP workflows.
- Structured audit logs are routed to a restricted Cloud Logging sink with the
  documented retention policy.
- The launch flow creates a realistic sandbox, connects the customer's agent,
  and produces a useful MCP-backed compliance answer without a sales gate.
