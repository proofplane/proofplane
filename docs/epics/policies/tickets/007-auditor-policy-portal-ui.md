# 007 - Auditor Policy Portal UI

**Status:** Todo · **Depends on:** [006](./006-auditor-policy-read-model-and-downloads.md) · **Spec:** [ux.md](../ux.md#auditor-navigation)

**Summary** - Give auditors a top-level policy catalog, policy details, and
per-control policy visibility inside the existing read-only portal.

**Acceptance criteria**

- [ ] Given a valid auditor session, when the Policies destination opens, then
  all active policies appear with description, mapping count, document state,
  deterministic ordering, and an intentional empty state when applicable.
- [ ] Given a policy row, when its detail opens, then full description, mapped
  controls, document state, and an eligible document download are shown.
- [ ] Given a control detail page, when it opens, then attached active policies
  are listed and linked, or an explicit empty state is shown.
- [ ] Given unavailable documents or untrusted policy content, when rendered,
  then status is clear, downloads are absent, and all user-authored text is
  escaped.
- [ ] Given the existing framework/control/evidence portal, when policy UI
  ships, then its routes and read-only behavior remain usable and unchanged.

**Tasks**

- [ ] Add accessible top-level Framework requirements/Policies navigation.
- [ ] Add server-rendered policy catalog and policy detail routes/pages.
- [ ] Add attached-policy sections to control detail pages.
- [ ] Link mapped controls deterministically, including controls without a
  framework requirement.
- [ ] Reuse portal layout, responsive, status, empty-state, and download
  patterns from `ux.md`.
- [ ] Add HTTP integration tests for navigation, list/detail/control states,
  ordering, escaping, downloads, empty states, and invalid sessions.
