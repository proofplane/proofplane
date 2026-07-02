# 007 - Auditor Portal UI

**Status:** Todo · **Depends on:** 003, 005, 006 · **Spec:** [ux.md](../ux.md)

**Summary** - Add the minimal browser experience for auditors using
server-rendered HTML from the Rust API instead of a separate frontend app.

**Acceptance criteria**

- [ ] Given an invite link, when opened, then the auditor can request and submit
  an OTP for the intended email.
- [ ] Given a valid session, when the portal opens, then it shows controls,
  mapped requests, submissions, attachment states, and eligible download
  actions.
- [ ] Given an expired, revoked, missing, or invalid session, when the portal
  opens, then it shows a recovery path without leaking workspace data.
- [ ] Given unavailable attachments, when displayed, then their state is clear
  and no download link is shown.

**Tasks**

- [ ] Add simple server-rendered HTML pages and minimal CSS.
- [ ] Add OTP, expired/revoked, portal, and attachment unavailable states.
- [ ] Add download links only for eligible attachments.
- [ ] Add keyboard and screen-reader friendly labels.
- [ ] Add HTTP integration tests for the main browser flows.
