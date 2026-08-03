# 002 - Agent Evidence Upload Coverage

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#agent-native-evidence-upload-coverage)

**Summary** - Restore black-box coverage for the shipped agent-native evidence
upload workflow from MCP preparation through raw HTTP transfer, polling, scan,
and finalization. Assertions stay at client, audit, and pipeline boundaries.

**Acceptance criteria**

- [ ] Given an authorized agent and valid declaration, when it prepares and
  transfers evidence bytes, then one attributed submission reaches `uploaded`
  and exposes the documented descriptor, projection, and success audits.
- [ ] Given invalid authority, concealed evidence, mismatched metadata, an
  interrupted stream, or an oversized body, when transfer is attempted, then
  the stable rejection occurs without an observable submission or false success
  audit and a safe retry remains possible where specified.
- [ ] Given matching retries, concurrent attempts, and the existing browser
  upload flow, when they run, then machine attempts converge on one result and
  human upload behavior remains unchanged.

**Tasks**

- [ ] Add a local helper that executes a returned machine transfer descriptor
  while leaving preparation arguments and authorization explicit in each test.
- [ ] Cover MCP descriptor shape, permissions, tenant concealment, evidence
  status eligibility, and declaration validation.
- [ ] Cover raw transfer authority, headers, length, checksum, body limit,
  interrupted-stream cleanup, and retry behavior through public responses.
- [ ] Cover matching replay and concurrent transfer convergence using complete
  `get_evidence_submission` projections and pipeline events.
- [ ] Assert agent provenance and exact secret-free grant/completion audits,
  including no false success records on rejected attempts.
- [ ] Run the focused evidence machine-upload tests alone and in the full
  integration-v2 target.

**Notes**

- The shipped behavior remains defined by the
  [Agent-Native Evidence Uploads spec](../../agent-native-evidence-uploads/spec.md).
