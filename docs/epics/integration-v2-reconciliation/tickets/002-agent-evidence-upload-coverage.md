# 002 - Agent Evidence Upload Coverage

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#agent-native-evidence-upload-coverage)

**Summary** - Restore black-box coverage for the shipped agent-native evidence
upload workflow from MCP preparation through raw HTTP transfer, polling, scan,
and finalization. Assertions stay at client, audit, and pipeline boundaries.

**Acceptance criteria**

- [x] Given an authorized agent and valid declaration, when it prepares and
  transfers evidence bytes, then one attributed submission reaches `uploaded`
  and exposes the documented descriptor, projection, and success audits.
- [x] Given invalid authority, concealed evidence, mismatched metadata, an
  interrupted stream, or an oversized body, when transfer is attempted, then
  the stable rejection occurs without an observable submission or false success
  audit and a safe retry remains possible where specified.
- [x] Given matching retries, concurrent attempts, and the existing browser
  upload flow, when they run, then machine attempts converge on one result and
  human upload behavior remains unchanged.

**Tasks**

- [x] Add a local helper that executes a returned machine transfer descriptor
  while leaving preparation arguments and authorization explicit in each test.
- [x] Cover MCP descriptor shape, permissions, tenant concealment, active
  evidence eligibility, and declaration validation; record unreachable status
  arrangements at the lower boundary.
- [x] Cover raw transfer authority, headers, length, checksum, body limit,
  interrupted-stream cleanup, and retry behavior through public responses.
- [x] Cover matching replay and concurrent transfer convergence using complete
  `get_evidence_submission` projections and pipeline events.
- [x] Assert agent provenance and exact secret-free grant/completion audits,
  including no false success records on rejected attempts.
- [x] Run the focused evidence machine-upload tests alone and in the full
  integration-v2 target.
- [x] Split the machine-upload coverage into story-focused directory modules
  and document the integration-v2 module-size convention.

**Notes**

- The shipped behavior remains defined by the
  [Agent-Native Evidence Uploads spec](../../agent-native-evidence-uploads/spec.md).
- The reconciliation spec records why paused/retired arrangements and
  deterministic storage/transaction failures remain lower-boundary gaps under
  integration-v2's strict client-only rule.
