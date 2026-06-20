# 004 - Auditor Packet Export Tools

**Status:** Todo · **Depends on:** 001, trusted-compliance-reads/005, trusted-compliance-reads/006 · **Spec:** [spec.md](../spec.md#context-efficient-results)

**Summary** - Let agents request and monitor asynchronous packet exports and
create human browser grants without transporting ZIP bytes through MCP.

**Acceptance criteria**

- [ ] Given a valid packet selection, when export is requested, then the result
  contains only export ID and status while the worker owns ZIP generation.
- [ ] Given a pending or building export, when status is read, then compact
  lifecycle metadata and bounded polling guidance are returned.
- [ ] Given a ready export, when a download grant is created, then the result
  identifies the URL as a bearer secret for human use and contains no ZIP bytes
  or object key.
- [ ] Given a failed, expired, missing, or cross-workspace export, when a status
  or grant tool is called, then a stable problem is returned without leaking
  dependency or object-storage details.
- [ ] Given any packet export tool result, when serialized, then ZIP bytes,
  attachment bytes, submission free text, and duplicated explanatory prose are
  absent.

**Tasks**

- [ ] Add packet-export request, status, and download-grant tools.
- [ ] Map service lifecycle/problems into compact MCP DTOs.
- [ ] Annotate export request as side-effecting and status/grant tools according
  to their established risk semantics.
- [ ] Add pending, ready, failed, expiry, tenant-isolation, and secret-exclusion
  integration tests.

**Notes**

- The direct grant response may contain the browser URL; the agent must present
  it to the user rather than fetching, logging, or persisting it.
