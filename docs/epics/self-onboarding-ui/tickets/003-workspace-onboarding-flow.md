# 003 - Workspace Onboarding Flow

**Status:** Todo · **Depends on:** 001, 002, auth-hierarchy-api/002 · **Spec:** [spec.md](../spec.md#routes)

**Summary** - Let an authenticated user create or resume a workspace setup, with
the sandbox path as the default so first-run users do not land in an empty app.

**Acceptance criteria**

- [ ] Given an authenticated user with no workspace, when they enter the app,
  then they are routed to workspace onboarding.
- [ ] Given a valid workspace name and sandbox selection, when submitted, then
  the UI creates the workspace and routes to token creation.
- [ ] Given the workspace create API rejects the request, when submission fails,
  then the UI shows the field or request error and preserves user input.
- [ ] Given an authenticated user with existing workspaces, when they enter the
  app, then they can select or resume a workspace without creating a duplicate.

**Tasks**

- [ ] Add typed API calls for `GET /workspaces` and `POST /workspaces`.
- [ ] Build onboarding route, workspace form, and sandbox/blank selector.
- [ ] Add loading, empty, conflict, and permission/not-found states.
- [ ] Invalidate workspace queries after create.
- [ ] Add Vitest coverage for route decisions and mutation errors.

**Notes**

- Backend sandbox seeding may be separate launch work. Until then, the UI should
  clearly label sandbox data that is sample or preview-only.
