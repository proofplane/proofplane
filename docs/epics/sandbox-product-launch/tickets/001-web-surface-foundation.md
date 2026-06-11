# 001 - Web Surface Foundation

**Status:** Todo · **Depends on:** release-hardening/002 · **Spec:** [spec.md](../spec.md#goal)

**Summary** - Choose and scaffold the browser application architecture, Auth0
session model, API client, shared styling, and deployment boundary.

**Acceptance criteria**

- [ ] Given the selected architecture, when a developer starts the local stack,
  then public pages and an authenticated shell are available with documented
  commands.
- [ ] Given an unauthenticated visitor, when a protected product route is opened,
  then login returns them to the intended route.
- [ ] Given invalid or expired browser auth, when an API call runs, then the UI
  clears the session safely and never logs tokens.
- [ ] Given API-only clients, when the web surface ships, then existing REST and
  MCP contracts remain unchanged.

**Tasks**

- [ ] Record the frontend framework/deployment decision in the spec revision log.
- [ ] Scaffold public and authenticated route groups.
- [ ] Add Auth0 browser session, CSRF, and API client handling.
- [ ] Add accessibility and responsive layout foundations.
- [ ] Add local startup and smoke tests.
