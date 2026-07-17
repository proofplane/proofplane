# 007 - Auditor Portal UI

**Status:** Done · **Depends on:** 003, 005, 006 · **Spec:** [ux.md](../ux.md)

**Summary** - Add the minimal browser experience for auditors using
server-rendered HTML from the Rust API instead of a separate frontend app.

**Acceptance criteria**

- [x] Given an invite link, when opened, then the auditor can request and submit
  an OTP for the intended email.
- [x] Given a valid session, when the portal opens, then it shows controls,
  mapped requests, submissions, submission states, and eligible download
  actions.
- [x] Given an expired, revoked, missing, or invalid session, when the portal
  opens, then it shows a recovery path without leaking workspace data.
- [x] Given unavailable submissions, when displayed, then their state is clear
  and no download link is shown.

**Tasks**

- [x] Add simple server-rendered HTML pages and minimal CSS.
- [x] Add OTP, expired/revoked, portal, and submission unavailable states.
- [x] Add download links only for eligible submissions.
- [x] Add keyboard and screen-reader friendly labels.
- [x] Add HTTP integration tests for the main browser flows.

**Notes**

- Spec revised to record the shipped server-rendered browser invite and portal
  routes.
