# 001 - Transport-Neutral Evidence Ingestion

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#existing-baseline)

**Summary** - Extract the reusable streaming and staging behavior from the
browser-specific adapter so both human multipart uploads and machine byte
streams use one quarantine, checksum, size-limit, and cleanup contract.

**Acceptance criteria**

- [x] Given a valid browser multipart upload, when the refactor ships, then its
  submission, provenance, scan enqueueing, and response behavior are unchanged.
- [x] Given any accepted stream, when it is staged, then length, SHA-256, and
  CRC32C are computed without buffering the complete file in memory.
- [x] Given a stream that exceeds the configured maximum or fails mid-transfer,
  when ingestion stops, then no submission is created and staged bytes are
  cleaned up.
- [x] Given invalid multipart input, when the browser route handles it, then its
  existing stable validation response is unchanged.

**Tasks**

- [x] Define a transport-neutral staged-evidence input and result boundary.
- [x] Move stream limiting, staging, checksums, and cleanup behind that boundary.
- [x] Adapt the browser multipart route without changing its public contract.
- [x] Add unit tests for pure stream validation and metadata decisions.
- [x] Preserve and extend browser upload integration coverage.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.
