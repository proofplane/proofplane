# 009 — Guide Documentation

**Status:** Done · **Depends on:** 002, 003, 004, 005, 006, 007, 008 · **Spec:** [spec.md](../spec.md#guide-updates)

**Summary** — Update the embedded MCP guide topics so agents know when to reach
for a batch tool over its single-pair sibling and that batches are atomic. An
agent that assumes partial application will write defensive retry logic that
re-applies mappings.

**Acceptance criteria**

- [x] Given the `controls-and-mappings` topic, when it is fetched, then it documents all four evidence ↔ control batch tools and states the all-or-nothing contract.
- [x] Given the `policies` topic, when it is fetched, then it documents all four policy ↔ control batch tools and states the all-or-nothing contract.
- [x] Given either topic, when it is fetched, then it states the 50-item cap and that a rejected batch writes nothing, so retrying the corrected batch is safe.
- [x] Given an agent reading either topic, when it looks for the one-way rule, then the topic explains that a batch fans out from one anchor and that the two directions are separate tools.
- [x] Given the guide topic index, when it is fetched, then its topic list is unchanged — no new topics are added.

**Tasks**

- [x] Update `src/mcp/docs/controls-and-mappings.md`.
- [x] Update `src/mcp/docs/policies.md`.
- [x] Verify the tool names and error codes in the docs match what shipped in 002–008.
- [x] Tests asserting both topics render and mention the batch tool names.

**Notes**

- Lands last so the documented names and error codes match the final
  implementation rather than the spec's proposal.
