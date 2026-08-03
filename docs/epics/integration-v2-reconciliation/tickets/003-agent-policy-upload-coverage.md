# 003 - Agent Policy Upload Coverage

**Status:** Todo · **Depends on:** 001, 002 · **Spec:** [spec.md](../spec.md#agent-native-policy-document-upload-coverage)

**Summary** - Restore black-box coverage for policy document machine uploads
while pinning the one-current-document invariant across retries and races. Reuse
only evidence transfer mechanics; keep policy authority and outcomes explicit.

**Acceptance criteria**

- [ ] Given an authorized agent and eligible policy, when it prepares and
  transfers a document, then one attributed current document reaches `uploaded`
  with the documented projection and secret-free audits.
- [ ] Given invalid authority or declaration, an unavailable policy, a current
  document, interrupted transfer, or oversized body, when transfer is
  attempted, then the stable rejection occurs without an unintended document,
  replacement, archive, or false success audit.
- [ ] Given retry and concurrency under one grant, competing machine grants,
  or a machine-versus-browser race, when uploads overlap, then one current
  document wins and human management behavior remains unchanged.

**Tasks**

- [ ] Add policy-specific MCP preparation and raw transfer stories using the
  shared descriptor transport helper from 002.
- [ ] Cover permissions, archived/missing/cross-workspace concealment, existing
  document conflict, and complete descriptor validation.
- [ ] Cover authority, headers, length, checksum, body limit, interruption,
  replay, and stable failure responses through public boundaries.
- [ ] Cover same-grant convergence, competing-grant conflict, and
  machine-versus-browser single-winner behavior.
- [ ] Assert `get_policy` lifecycle and provenance plus exact secret-free
  grant/completion audits and absence of false success records.
- [ ] Run the focused policy machine-upload tests alone and in the full
  integration-v2 target, including existing browser management regressions.

**Notes**

- The shipped behavior remains defined by the
  [Agent-Native Policy Document Uploads spec](../../agent-native-policy-document-uploads/spec.md).
