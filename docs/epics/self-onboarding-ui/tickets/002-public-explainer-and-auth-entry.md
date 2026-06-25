# 002 - Public Explainer And Auth Entry

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#routes)

**Summary** - Build the public entry screen that explains Proofplane as
AI-native SOC 2 infrastructure and sends users into Auth0 without a sales gate.

**Acceptance criteria**

- [x] Given an unauthenticated visitor, when they open `/`, then they understand
  what Proofplane is, what the sandbox creates, and how to start.
- [x] Given the primary CTA, when clicked, then the visitor is sent to Auth0
  signup/login and returns through the configured callback.
- [x] Given Auth0 is misconfigured or unavailable, when auth starts or returns,
  then the UI shows a recoverable error without losing the public page.
- [x] Given the product positioning, when this ships, then "Book a Demo" is not
  the primary path.

**Tasks**

- [x] Add Auth0 React SDK configuration and callback handling.
- [x] Build `/` using the `Audit Workbench` visual system.
- [x] Add primary `Start SOC 2 Sandbox` CTA and secondary pricing/docs links.
- [x] Add auth error and loading states.
- [x] Add component tests for CTA behavior and error states.

**Notes**

- This ticket only covers browser auth entry. Auth0 validation and JIT user
  provisioning already live in the Auth Hierarchy API epic.
- Verified with `npm run build`, `npm test`, and `npm run test:smoke`.
