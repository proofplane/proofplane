# Self-Onboarding UI Spec

## Goal

Build the minimal Proofplane web UI that lets a founder understand the product,
sign in, create a workspace, issue a scoped API token, and see how MCP-backed
agent workflows will connect to Proofplane data.

This epic is frontend-first. It does not implement Auth0, workspace, token, MCP,
or auditor portal backend behavior. It consumes the existing management APIs and
shows clear preview or unavailable states for backend capabilities still in
progress.

## Product Context

The source product context lives in [PRODUCT.md](../../../PRODUCT.md). The UI
must preserve these principles:

- humans manage workspaces and credentials;
- actors and agents use scoped credentials for data-plane work;
- the first-run path should produce a visible workspace, not an empty dashboard;
- permission choices must be understandable before a token is issued;
- MCP work that is not production-ready must be labeled honestly.

The source visual context lives in [DESIGN.md](../../../DESIGN.md) and
[DESIGN.json](../../../DESIGN.json). This epic uses the seeded "Audit Workbench"
system until real UI code exists and can be re-documented.

## Stack

Use the smallest stack that covers the product requirements:

- Vite + React + TypeScript for the app.
- React Router for public, auth callback, onboarding, workspace, and settings
  routes.
- Auth0 React SDK for browser authentication.
- TanStack Query for server state, loading states, errors, retries, and cache
  invalidation.
- Plain CSS with CSS variables derived from `DESIGN.md`.
- lucide-react for icons.
- Vitest for small component and state tests.
- Playwright for the onboarding smoke test.

Do not add Redux, Zustand, Tailwind, shadcn, Next.js, or an OpenAPI generator in
this epic. Add Radix primitives only if a native element cannot meet the
accessibility requirement.

## Application Boundary

The UI lives in the separate sibling `landing-portal` repository:

```text
landing-portal/
  src/
    api/
    auth/
    components/
    routes/
    styles/
  package.json
  vite.config.ts
```

Local development should run the Rust API and Vite UI as separate processes.
Production static serving is deferred until deployment planning requires it.

The UI reads its API base URL and Auth0 values from Vite environment variables.
No secrets belong in the browser bundle.

## Routes

Public routes:

- `/` explains Proofplane and offers the primary workspace setup CTA.
- `/pricing` may be a simple placeholder with public pricing philosophy if
  detailed pricing is not ready.

Authenticated routes:

- `/app/onboarding` creates or resumes first-run setup.
- `/app/workspaces/:workspaceId` shows the workspace home.
- `/app/workspaces/:workspaceId/tokens` lists issued tokens and supports revoke.
- `/app/workspaces/:workspaceId/settings` shows workspace identity and members
  when APIs exist.

Auth routes:

- `/auth/callback` handles Auth0 return.

## API Client

Start with a small handwritten API client in `landing-portal/src/api/`. The client should:

- attach the Auth0 access token for management-plane calls;
- return typed DTOs for the routes the UI consumes;
- normalize expected API errors into simple UI states;
- let TanStack Query own caching, refetch, and mutation invalidation.

Do not generate a TypeScript API client in this epic. Revisit OpenAPI generation
only if the REST surface grows enough that handwritten DTOs repeatedly drift.

## Token Permission Model

Token creation must avoid a raw checkbox dump. Present job-based presets and
show the underlying granular permissions before submission.

Initial presets:

- **Read compliance data:** read evidence requests, evidence submissions, and
  controls.
- **Submit evidence:** read evidence requests and write evidence submissions.
- **Manage mappings:** read/write controls and read/write evidence requests when
  supported by the API contract.
- **Custom:** granular permission selection.

The UI must make clear that the raw token is shown once. It must not persist the
raw token outside the in-memory response and the visible one-time success state.

## MCP Setup Preview

MCP is still being worked on in a separate branch. The UI should include an MCP
setup preview after token creation and in the workspace home. It should explain:

- what MCP will let agents do;
- that the bearer token is supplied to the MCP session, not tool arguments;
- suggested prompts for the first demo;
- which capabilities are ready, preview-only, or waiting on backend tickets.

The preview may include copyable config snippets, but it must not claim that MCP
is production-ready until the MCP Server epic says so.

## Workspace Home

The workspace home should not be an empty dashboard. It should show:

- workspace identity and setup progress;
- starter SOC 2 controls;
- sample evidence request status;
- token/MCP readiness;
- suggested agent prompts;
- an auditor portal access area with a clear unavailable or preview state until
  Auditor Portal Access is ready.

Use real API data where available. Do not show sample setup modes until the
backend supports them.

## Error And Empty States

The UI must include clear recovery paths for:

- unauthenticated user;
- authenticated user with no workspace;
- workspace creation failure;
- token creation failure;
- raw token response shown once and not saved;
- revoked or missing token;
- MCP unavailable;
- auditor portal access unavailable;
- permission denied or not found responses from the API.

## Testing

Keep tests small:

- Vitest for permission preset behavior, API error normalization, and token
  one-time display logic.
- Playwright for one first-run smoke test that covers public CTA, authenticated
  onboarding with mocked Auth0/API, workspace creation, token creation, and MCP
  setup visibility.
- Accessibility checks should cover keyboard navigation, focus visibility, and
  non-color-only status labels in the smoke flow.

## Revisions

- 2026-06-23: Initial scope from `PRODUCT.md`, `DESIGN.md`, the agreed ponytail
  stack, and the then-current Auth/MCP compliance epics.
- 2026-07-01: Replaced stale auditor planning references with Auditor Portal
  Access.
