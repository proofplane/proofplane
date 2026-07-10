# 008 - Worker OTP Email Delivery

**Status:** Todo · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#deferred-work)

**Summary** - Move auditor OTP email delivery behind the worker when
production mail is added, so request handling does not depend on provider
latency and delivery can use the existing retry path.

**Acceptance criteria**

- [ ] Given a valid invite, when the auditor requests an OTP, then Proofplane
  queues worker-owned OTP creation and mail delivery without storing the raw
  OTP in outbox payloads.
- [ ] Given provider latency or retryable failure, when delivery is attempted,
  then the worker retries without blocking the auditor request handler.
- [ ] Given disabled or misconfigured production mail, when OTP delivery is
  requested, then the failure is observable without exposing OTP secrets.

**Tasks**

- [ ] Add a worker message for auditor OTP email requests.
- [ ] Move OTP code generation, digest persistence, and mail send into the
  worker handler.
- [ ] Wire the production mail adapter into the worker binary.
- [ ] Add integration coverage for queued delivery and retry behavior.

**Notes**

- Keep the raw OTP out of durable queue payloads; the worker must generate it.
