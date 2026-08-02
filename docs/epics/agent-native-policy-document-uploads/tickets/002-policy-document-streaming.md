# 002 - Policy Document Streaming

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#streaming-endpoint)

**Summary** - Add the raw PUT endpoint that streams a declared policy document
through quarantine and atomically creates one pending document. Harden
completion so retries and policy-document races remain deterministic.

**Acceptance criteria**

- [x] Given a valid grant and matching stream, when the PUT completes, then it
  returns `201` with the policy ID, document ID, and `pending` status and queues
  one scan event.
- [x] Given a matching retry after completion, when it arrives before expiry,
  then it returns `200` with the original document and creates nothing new.
- [x] Given concurrent attempts under one grant, when both reach completion,
  then one document wins and all losing quarantine objects are deleted.
- [x] Given another machine or browser transfer creates the current document,
  when this attempt completes, then it receives a stable conflict without
  replacing the winner or emitting another scan event.
- [x] Given unavailable authority, invalid metadata, an interrupted stream, or
  dependency failure, when the request ends, then no partial document commits
  and cleanup preserves a safe retry where applicable.
- [x] Given the human browser flow, when the endpoint ships, then its upload,
  archive, download, and replacement behavior is unchanged.

**Tasks**

- [x] Register the policy-specific PUT route with configured request limits.
- [x] Verify policy upload authority, path ID, headers, and declaration before
  completion.
- [x] Stream each attempt through shared quarantine staging without full-body
  buffering.
- [x] Implement row-locked document, outbox, and grant completion.
- [x] Implement matching replay, single-grant races, cross-grant races, and
  current-document conflict cleanup.
- [x] Add HTTP and Postgres integration tests for success, rejection, rollback,
  provenance, scanner handoff, replay, races, and cleanup.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.

**Notes**

- The spec revision records that authority is verified before request-size
  rejection and that both route and service streaming limits remain active.
