# 003 - Production Pub/Sub Startup

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#pubsub-runtime)

**Summary** - Let the dequeuer provision and publish through authenticated
Google Pub/Sub when no emulator is configured.

**Acceptance criteria**

- [ ] Given no emulator variable and valid application default credentials,
  when dequeuer starts, then topics/subscription are provisioned and publishing
  begins.
- [ ] Given missing credentials or denied provisioning, when dequeuer starts,
  then it exits with an actionable error and does not poll outbox rows.
- [ ] Given the emulator variable, when local tests run, then existing emulator
  provisioning and delivery behavior is unchanged.

**Tasks**

- [ ] Remove the emulator-required startup guard.
- [ ] Make client mode selection explicit in startup logs without secrets.
- [ ] Preserve idempotent topic and subscription provisioning.
- [ ] Add construction/error tests and retain emulator integration coverage.
- [ ] Document deployment-level push endpoint protection.
