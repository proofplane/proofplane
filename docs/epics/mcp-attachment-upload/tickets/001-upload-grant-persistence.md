# 001 - Upload Grant Persistence

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#upload-grant-model)

**Summary** - Add durable single-use attachment upload grants so a browser URL
can be redeemed exactly once before it becomes a scoped upload session.

**Acceptance criteria**

- [x] Given an authorized workspace actor, when an upload grant is issued for an
  existing submission, then a persisted grant records workspace, submission,
  issuing user, issuing API token, and expiry.
- [x] Given a grant is redeemed, when another redemption is attempted, then the
  second redemption fails without exposing whether the grant ever existed.
- [x] Given a grant is expired, malformed, missing, or cross-workspace, when it
  is redeemed, then it returns the generic unavailable result.
- [x] Given existing attachment download grants, when this ships, then their
  reusable five-minute behavior is unchanged.

**Tasks**

- [x] Add a migration for persisted upload grants.
- [x] Add repository methods to create and atomically redeem upload grants.
- [x] Add a service layer for issuing and redeeming upload grants.
- [x] Add encrypted/authenticated URL token handling with upload-specific
  purpose separation.
- [x] Add unit and integration tests for expiry, single-use redemption, and
  unchanged download-grant behavior.

**Notes**

- Use Postgres for single-use state; do not add Redis or an in-memory cache.
- The implementation adds a dedicated `paseto.upload_grant` keyring rather than
  reusing the download-grant keyring.
