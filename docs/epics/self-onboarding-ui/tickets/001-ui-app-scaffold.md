# 001 - UI App Scaffold

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#stack)

**Summary** - Add the minimal Vite React TypeScript app under `ui/`, wire the
design tokens from `DESIGN.md`, and establish the routing, API, and test
foundation for the onboarding UI.

**Acceptance criteria**

- [x] Given the repository, when a developer runs the documented UI dev command,
  then the Vite app starts and renders the app shell.
- [x] Given the seeded design system, when the app renders, then CSS variables
  for colors, typography, spacing, and radii match `DESIGN.md`.
- [x] Given an unknown route, when the user navigates to it, then the UI shows a
  recoverable not-found state.
- [x] Given the Rust crate, when this ships, then existing `make check` behavior
  is unchanged.

**Tasks**

- [x] Create `ui/` with Vite, React, TypeScript, and React Router.
- [x] Add plain CSS token files from `DESIGN.md`.
- [x] Add app shell, route layout, not-found route, and base components.
- [x] Add TanStack Query provider and a small handwritten API client skeleton.
- [x] Add Vitest and Playwright configuration with one render smoke test.
- [x] Document UI dev/test commands.

**Notes**

- Per the stack decision, do not add Tailwind, shadcn, Redux, Zustand, Next.js,
  or generated API clients in this ticket.
- Verified with `npm run build`, `npm test`, `npm run test:smoke`,
  `npm audit --audit-level=moderate`, and `make check`.
