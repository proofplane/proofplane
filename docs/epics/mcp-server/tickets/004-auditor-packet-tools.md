# 004 - Auditor Packet Tools

**Status:** Todo · **Depends on:** 001, trusted-compliance-reads/004, trusted-compliance-reads/005, trusted-compliance-reads/006 · **Spec:** [spec.md](../spec.md#auditor-packet-tools)

**Summary** - Let agents preview packet readiness, request and monitor
asynchronous exports, and create human browser grants without transporting ZIP
bytes through MCP.

**Acceptance criteria**

- [ ] Given selected controls, when packet preview is requested, then compact
  readiness states and gaps are returned without submission free text.
- [ ] Given a valid packet selection, when export is requested, then the result
  contains only export ID and status while the worker owns ZIP generation.
- [ ] Given a pending or building export, when status is read, then compact
  lifecycle metadata and bounded polling guidance are returned.
- [ ] Given a ready export, when a download grant is created, then the result
  identifies the URL as a bearer secret for human use and contains no ZIP bytes
  or object key.
- [ ] Given a failed, expired, missing, or cross-workspace export, when a packet
  tool is called, then a stable problem is returned without leaking dependency
  or object-storage details.

**Tasks**

- [ ] Add packet-preview, export-request, status, and download-grant tools.
- [ ] Map service readiness/lifecycle/problems into compact MCP DTOs.
- [ ] Annotate export request as side-effecting and the remaining tools
  according to their established risk semantics.
- [ ] Add readiness, pending, ready, failed, expiry, tenant-isolation, and
  secret-exclusion integration tests.

**Notes**

- This ticket is not required for the core MCP demo.
- The direct grant response may contain the browser URL; the agent must present
  it to the user rather than fetching, logging, or persisting it.
