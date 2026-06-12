# 005 - Auditor Packet Export

**Status:** Todo · **Depends on:** 004 · **Spec:** [spec.md](../spec.md#auditor-packet-read-model)

**Summary** - Stream an auditor-ready ZIP containing a machine-readable
manifest, readable summary, and only finalized attachment bytes.

**Acceptance criteria**

- [ ] Given a valid packet selection, when export runs, then the ZIP contains a
  JSON manifest, Markdown summary, and every eligible uploaded attachment.
- [ ] Given pending, malicious, failed, missing, or integrity-mismatched objects,
  when export runs, then they are excluded with an explicit manifest gap or the
  export fails before serving a corrupt packet.
- [ ] Given duplicate filenames, when export runs, then archive paths are stable
  and collision-free.
- [ ] Given a successful export, when structured logs are inspected, then one
  packet export audit log is present without attachment bytes.

**Tasks**

- [ ] Add streaming ZIP assembly and stable archive paths.
- [ ] Reuse attachment eligibility and metadata verification.
- [ ] Add export route, headers, and audit log.
- [ ] Add archive-content and failure integration tests.
