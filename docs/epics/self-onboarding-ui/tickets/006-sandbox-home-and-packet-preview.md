# 006 - Sandbox Home And Packet Preview

**Status:** Todo · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#sandbox-home)

**Summary** - Build the authenticated sandbox home that shows setup progress,
starter controls, evidence request status, token/MCP readiness, suggested
prompts, and packet preview or unavailable states.

**Acceptance criteria**

- [ ] Given a workspace with available compliance data, when the home renders,
  then controls, evidence request status, and readiness are shown from API data.
- [ ] Given packet preview APIs are unavailable, when the home renders, then the
  packet area clearly states what is waiting and links to the relevant setup.
- [ ] Given preview/sample data is used, when it appears, then it is labeled as
  sample or preview data.
- [ ] Given a permission/not-found API response, when data loads, then the UI
  shows a recoverable state without leaking cross-workspace details.
- [ ] Given existing evidence APIs, when this ships, then data-plane behavior is
  not changed by the UI.

**Tasks**

- [ ] Add typed reads for the currently available controls/evidence/packet APIs.
- [ ] Build sandbox home layout and setup progress.
- [ ] Build starter controls/evidence status sections.
- [ ] Build auditor packet preview, unavailable, and sample states.
- [ ] Add Playwright first-run smoke coverage through sandbox home.

**Notes**

- Replace sample states with real Trusted Compliance Reads data as those tickets
  land; do not block the UI shell on packet export.
