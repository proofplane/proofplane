# 001 - Web Surface Foundation

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#goal)

**Summary** - Choose and scaffold the minimal browser architecture for public
content, Auth0 login, sandbox provisioning, and MCP credential setup.

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
- [ ] Given the authenticated browser surface, when its routes are inventoried,
  then it does not include compliance CRUD or embedded-agent chat pages.

**Tasks**

- [ ] Record the frontend framework/deployment decision in the spec revision log.
- [ ] Scaffold public routes and the minimal authenticated setup routes.
- [ ] Add Auth0 browser session, CSRF, and API client handling.
- [ ] Add accessibility and responsive layout foundations.
- [ ] Add local startup and smoke tests.
