# 002 - Public Explainer And Auth Entry

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#routes)

**Summary** - Build the public entry screen that explains Proofplane as
AI-native SOC 2 infrastructure and sends users into Auth0 without a sales gate.

**Acceptance criteria**

- [ ] Given an unauthenticated visitor, when they open `/`, then they understand
  what Proofplane is, what the sandbox creates, and how to start.
- [ ] Given the primary CTA, when clicked, then the visitor is sent to Auth0
  signup/login and returns through the configured callback.
- [ ] Given Auth0 is misconfigured or unavailable, when auth starts or returns,
  then the UI shows a recoverable error without losing the public page.
- [ ] Given the product positioning, when this ships, then "Book a Demo" is not
  the primary path.

**Tasks**

- [ ] Add Auth0 React SDK configuration and callback handling.
- [ ] Build `/` using the `Audit Workbench` visual system.
- [ ] Add primary `Start SOC 2 Sandbox` CTA and secondary pricing/docs links.
- [ ] Add auth error and loading states.
- [ ] Add component tests for CTA behavior and error states.

**Notes**

- This ticket only covers browser auth entry. Auth0 validation and JIT user
  provisioning already live in the Auth Hierarchy API epic.
