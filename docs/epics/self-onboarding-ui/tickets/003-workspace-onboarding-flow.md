# 003 - Workspace Onboarding Flow

**Status:** Done · **Depends on:** 001, 002, auth-hierarchy-api/002 · **Spec:** [spec.md](../spec.md#routes)

**Summary** - Let an authenticated user create or resume a workspace using the
current backend workspace APIs.

**Acceptance criteria**

- [x] Given an authenticated user with no workspace, when they enter the app,
  then they are routed to workspace onboarding.
- [x] Given a valid workspace name, when submitted, then
  the UI creates the workspace and routes to token creation.
- [x] Given the workspace create API rejects the request, when submission fails,
  then the UI shows the field or request error and preserves user input.
- [x] Given an authenticated user with existing workspaces, when they enter the
  app, then they can select or resume a workspace without creating a duplicate.

**Tasks**

- [x] Add typed API calls for `GET /workspaces` and `POST /workspaces`.
- [x] Build onboarding route and workspace form.
- [x] Add loading, empty, conflict, and permission/not-found states.
- [x] Invalidate workspace queries after create.
- [x] Add Vitest coverage for route decisions and mutation errors.

**Notes**

- The backend currently accepts workspace `name`/`slug` only. Do not show
  setup-mode choices until the API supports them.
