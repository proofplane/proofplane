# 002 - CQRS Application Foundation

**Status:** Done · **Triage:** ready-for-agent · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#commands-queries-and-execution-metadata)

**Summary** - Introduce concrete command/query handler conventions and prove
them by converting machine evidence-upload-grant issuance as the reference
slice.

**Acceptance criteria**

- [x] Given an authorized connection and eligible evidence, when issuance is handled, then a complete grant snapshot and credential are returned.
- [x] Given missing or cross-workspace evidence, when issuance is handled, then concealed unavailability is returned and no grant is saved.
- [x] Given existing HTTP and MCP clients, when the handler replaces the service operation, then their contracts are unchanged.

**Tasks**

- [x] Add application command, query, metadata, and handler module conventions.
- [x] Implement the issuance command and concrete handler.
- [x] Wire routes and MCP callers to the typed handler.
- [x] Retain only a temporary delegating compatibility boundary where needed.
- [x] Add unit and integration tests and run focused checks.
