# 006 - Auditor Packet Download Grants

**Status:** Todo · **Depends on:** 005 · **Spec:** [spec.md](../spec.md#packet-download-grants)

**Summary** - Issue short-lived browser download URLs for ready packet exports
and stream the persisted ZIP through Proofplane so agents only handle compact
status and grant metadata.

**Acceptance criteria**

- [ ] Given a ready unexpired export, when a grant is requested, then a
  five-minute packet-specific PASETO URL is returned without exposing its object
  key or ZIP bytes.
- [ ] Given a pending, building, failed, expired, missing, or cross-workspace
  export, when a grant is requested, then it is rejected without issuing a URL.
- [ ] Given a valid grant, when a browser opens it, then Proofplane rechecks the
  export and object metadata and streams the ZIP with safe download headers.
- [ ] Given a malformed, expired, tampered, or mismatched grant, when redeemed,
  then `404` is returned without revealing export existence.
- [ ] Given logs and MCP results, when inspected, then grant tokens, URLs,
  object keys, ZIP bytes, attachment bytes, and submission free text are absent
  except for the URL in the direct grant response.

**Tasks**

- [ ] Add packet-specific encrypted grant claims and validation.
- [ ] Add grant issuance and unauthenticated redemption services/routes.
- [ ] Reuse safe streaming headers and object-integrity checks.
- [ ] Add expiry and object-storage lifecycle configuration/documentation.
- [ ] Add grant, redemption, tenant-isolation, and secret-exclusion tests.
