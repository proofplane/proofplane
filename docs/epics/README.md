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
must be malware-scanned and finalized, source material is curated with
provenance, and freshness is derived from linked evidence.

## Current Reality

| Area | Implemented | Remaining |
| --- | --- | --- |
| Platform foundation | Rust runtimes, Postgres, config, logging, health routes, Pub/Sub emulator, outbox, worker | Production Pub/Sub startup, dependency hardening, application metrics |
| Identity and authorization | Auth0 users, workspace membership, user-owned opaque API tokens, actor retirement | Identity audit logs |
| Compliance model | Frameworks, controls, Evidence Requests, mappings | Trusted source material and auditor packet reads |
| Evidence lifecycle | Submission create/get, upload integrity, quarantine, ClamAV scan, finalization | Latest API, download enforcement, demo submission/object seed |
| Audit | Structured application logging | Stable audit-log fields, Cloud Logging retention, business event coverage |
| Agent interface | MCP binary scaffold | MCP runtime and tools |
| Launch surface | Product and GTM notes | Minimal sandbox/MCP setup UI and marketing site |

## Epic Portfolio

| Epic | Status | Outcome |
| --- | --- | --- |
| [Auth Hierarchy API](./auth-hierarchy-api/README.md) | Doing | Humans manage workspace actors and rotating API keys. |
| [API Token And PASETO Migration](./paseto-token-migration/README.md) | Todo | Users authenticate with compact workspace-scoped API tokens and download grants use encrypted PASETO. |
| [Evidence Lifecycle Completion](./evidence-lifecycle-completion/README.md) | Todo | Evidence can be queried, safely downloaded, and demonstrated end to end. |
| [Production Runtime Adapters](./production-runtime-adapters/README.md) | Todo | GCS and production Google Pub/Sub work without emulator-only assumptions. |
| [Trusted Compliance Reads](./trusted-compliance-reads/README.md) | Todo | Curated source material and auditor-ready packets expose provenance and freshness. |
| [MCP Server](./mcp-server/README.md) | Todo | Agents use the same services and authorization model as REST clients. |
| [Reliability and Observability](./reliability-observability/README.md) | Todo | Dependency failures and runtime behavior are visible and tested. |
| [Sandbox Product Launch](./sandbox-product-launch/README.md) | Todo | A founder can connect an agent to a realistic SOC 2 sandbox. |

## Preferred Sequence

1. Finish the Evidence Lifecycle Completion epic.
2. Complete `paseto-token-migration/006` before MCP authentication is built;
   its attachment-grant ticket can proceed in parallel.
3. Establish the structured audit-log contract in Reliability and
   Observability; identity and data-plane emission can then proceed in parallel.
4. Build production adapters while Trusted Compliance Reads starts on the
   completed evidence model.
5. Build MCP after user API-token authentication and trusted-read contracts are
   stable.
6. Add reliability coverage and metrics continuously alongside product work.
7. Build the Sandbox Product Launch on the stable APIs and packet preview.
8. Create a separate production-deployment epic when deployment planning begins.

## Definition Of MVP Done

- Every ticket in the backend epics is `Done`, with specs reconciled to what
  shipped.
- `make check` passes, and Docker-backed integration tests cover the release
  flow and external dependency failures.
- Production configuration supports Postgres, SpiceDB, Google Pub/Sub, GCS,
  Auth0, and ClamAV without local emulator requirements.
- User-owned API tokens can submit evidence, retrieve only finalized
  attachments, query trusted compliance material, and perform the supported MCP
  workflows.
- Structured audit logs are routed to a restricted Cloud Logging sink with the
  documented retention policy.
- The launch flow creates a realistic sandbox, connects the customer's agent,
  and produces a useful MCP-backed compliance answer without a sales gate.
