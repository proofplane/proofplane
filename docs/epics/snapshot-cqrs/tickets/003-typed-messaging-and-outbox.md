# 003 - Typed Messaging and Outbox

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#typed-integration-messaging-and-outbox)

**Summary** - Replace raw JSON outbox construction with a closed, versioned
message contract while retaining compatibility with queued legacy document
work.

**Acceptance criteria**

- [ ] Given a new scan or finalization command, when persisted and dequeued, then all envelope metadata and typed payload fields are preserved.
- [ ] Given a queued legacy scan or finalization envelope, when consumed, then it retains its previous behavior.
- [ ] Given an unknown version/type or malformed payload, when consumed, then it is acknowledged without state changes.
- [ ] Given snapshot persistence fails, when a command would enqueue follow-up work, then neither side commits.

**Tasks**

- [ ] Add closed integration command/event types and serializers.
- [ ] Add typed outbox columns and constraints to the baseline schema.
- [ ] Update producers, dequeuer, and worker dual decoding.
- [ ] Propagate correlation and causation identifiers.
- [ ] Add codec golden and atomicity integration tests.

**Notes** - The pre-deployment migration history was consolidated into V001;
see the 2026-08-02 revision in the spec.
