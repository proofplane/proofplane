# 008 - Policy Guidance And Demo Data

**Status:** Done · **Depends on:** [002](./002-mcp-policy-catalog-tools.md), [004](./004-mcp-policy-document-grants.md), [007](./007-auditor-policy-portal-ui.md) · **Spec:** [spec.md](../spec.md#guidance-and-demo-data)

**Summary** - Make the completed policy workflow discoverable to connected
agents and visible in local/demo workspaces without inflating initialization
context.

**Acceptance criteria**

- [x] Given a newly connected agent, when it inspects policy tool descriptions
  or requests the relevant Proofplane guide, then it can distinguish policies,
  controls, mappings, and the human-browser document workflow.
- [x] Given seeded local data, when compliance and auditor surfaces list
  policies, then representative mapped, unmapped, described, and document-less
  policy states are visible deterministically.
- [x] Given guidance text, seed output, logs, and fixtures, when inspected, then
  they include no bearer URL, credential, session secret, object key, or
  production configuration.
- [x] Given existing MCP guidance topics and demo controls/evidence, when policy
  guidance and seed data ship, then their contracts and identifiers remain
  unchanged.

**Tasks**

- [x] Update concise MCP tool descriptions and the relevant embedded guide
  topic(s) with policy vocabulary and workflow order.
- [x] Update guidance/tool catalog tests without duplicating the epic spec in
  model context.
- [x] Add deterministic demo policies and control mappings; keep binary policy
  object seeding out unless it can use the real lifecycle safely.
- [x] Update seed output and applicable API/MCP fixtures or docs.
- [x] Add seed idempotency and representative guidance integration tests.

**Notes**

- 2026-07-19: The spec now records the shipped policy guide topic and
  deterministic local policy fixtures.
