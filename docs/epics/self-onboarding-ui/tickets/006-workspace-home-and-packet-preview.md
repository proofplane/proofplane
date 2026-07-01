# 006 - Workspace Home And Auditor Access Preview

**Status:** Done - Will Do Later · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#workspace-home)

**Summary** - Build the authenticated workspace home that shows setup progress,
starter controls, evidence request status, token/MCP readiness, suggested
prompts, and auditor access preview or unavailable states.

**Acceptance criteria**

- [ ] Given a workspace with available compliance data, when the home renders,
  then controls, evidence request status, and readiness are shown from API data.
- [ ] Given auditor access APIs are unavailable, when the home renders, then the
  auditor access area clearly states what is waiting and links to the relevant
  setup.
- [ ] Given preview/sample data is used, when it appears, then it is labeled as
  sample or preview data.
- [ ] Given a permission/not-found API response, when data loads, then the UI
  shows a recoverable state without leaking cross-workspace details.
- [ ] Given existing evidence APIs, when this ships, then data-plane behavior is
  not changed by the UI.

**Tasks**

- [ ] Add typed reads for the currently available controls/evidence/auditor
  access APIs.
- [ ] Build workspace home layout and setup progress.
- [ ] Build starter controls/evidence status sections.
- [ ] Build auditor access preview, unavailable, and sample states.
- [ ] Add Playwright first-run smoke coverage through workspace home.

**Notes**

- Replace sample states with real Auditor Portal Access data as those tickets
  land; do not block the UI shell on portal backend work.
- Postponed until the MCP is more feature complete; revalidate the linked spec
  and UX before reopening because the current requirements may no longer apply.
