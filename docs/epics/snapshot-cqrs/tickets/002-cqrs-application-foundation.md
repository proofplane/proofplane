# 002 - CQRS Application Foundation

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#commands-queries-and-execution-metadata)

**Summary** - Introduce concrete command/query handler conventions and prove
them by converting machine evidence-upload-grant issuance as the reference
slice.

**Acceptance criteria**

- [ ] Given an authorized connection and eligible evidence, when issuance is handled, then a complete grant snapshot and credential are returned.
- [ ] Given missing or cross-workspace evidence, when issuance is handled, then concealed unavailability is returned and no grant is saved.
- [ ] Given existing HTTP and MCP clients, when the handler replaces the service operation, then their contracts are unchanged.

**Tasks**

- [ ] Add application command, query, metadata, and handler module conventions.
- [ ] Implement the issuance command and concrete handler.
- [ ] Wire routes and MCP callers to the typed handler.
- [ ] Retain only a temporary delegating compatibility boundary where needed.
- [ ] Add unit and integration tests and run focused checks.
