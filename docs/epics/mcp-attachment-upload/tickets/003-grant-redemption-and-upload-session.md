# 003 - Grant Redemption And Upload Session

**Status:** Todo · **Depends on:** [001](./001-upload-grant-persistence.md) · **Spec:** [spec.md](../spec.md#browser-session-model)

**Summary** - Redeem a one-time upload grant URL into an HttpOnly browser
session that can list and upload attachments for exactly one Evidence
Submission.

**Acceptance criteria**

- [ ] Given a valid unredeemed grant URL, when the browser opens it, then the
  grant is consumed and an HttpOnly upload-session cookie is set.
- [ ] Given the same grant URL is opened again, when redemption has already
  happened, then the response shows the generic unavailable state.
- [ ] Given a valid upload-session cookie, when the browser requests upload
  routes, then access is limited to the grant's scoped submission.
- [ ] Given an expired, malformed, missing, or wrong-scope session, when upload
  routes are requested, then no submission existence detail is leaked.

**Tasks**

- [ ] Add routes for grant redemption and session-backed upload access.
- [ ] Add upload-session token or cookie signing with a 15-minute fixed expiry.
- [ ] Scope the cookie path and set HttpOnly/SameSite attributes.
- [ ] Add submission attachment inventory loading through the session scope.
- [ ] Add integration tests for first redemption, repeat redemption, expiry,
  cookie attributes, and scoped access.

**Notes**

- The browser session is a narrow delegated credential, not a product login.
