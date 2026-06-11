# 003 - First-Run Product Flow

**Status:** Todo · **Depends on:** 001, 002, trusted-compliance-reads/004 · **Spec:** [spec.md](../spec.md#product-api-gaps)

**Summary** - Guide a workspace owner through editing a control, creating and
mapping an Evidence Request, and viewing a real packet preview.

**Acceptance criteria**

- [ ] Given a newly provisioned sandbox, when opened, then realistic starter
  records and a three-step checklist are shown instead of an empty dashboard.
- [ ] Given valid form input, when a control, request, and mapping are saved,
  then real backend records update and checklist progress survives refresh.
- [ ] Given invalid or conflicting input, when submitted, then field-level errors
  are accessible and no false completion is shown.
- [ ] Given the mapped records, when preview is opened, then the packet shows
  provenance and explicit missing-evidence gaps.

**Tasks**

- [ ] Build sandbox overview and record-backed checklist.
- [ ] Build control and Evidence Request forms.
- [ ] Build mapping interaction and packet preview.
- [ ] Add loading, empty, error, and retry states.
- [ ] Add browser smoke coverage for the complete first-run flow.

**Notes**

- Interface behavior is specified in [ux.md](../ux.md#first-run-flow).
