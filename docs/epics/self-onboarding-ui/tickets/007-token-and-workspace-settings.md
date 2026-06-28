# 007 - Token And Workspace Settings

**Status:** Done - Will Do Later · **Depends on:** 003, 004 · **Spec:** [spec.md](../spec.md#routes)

**Summary** - Add the minimal authenticated settings surfaces for workspace
identity, token listing, and token revocation so users can recover from setup
mistakes.

**Acceptance criteria**

- [ ] Given a workspace owner/admin, when they open token settings, then issued
  tokens are listed without raw secret values.
- [ ] Given a live token, when the user revokes it, then the list updates and
  the revoked token can no longer be used by the UI.
- [ ] Given revoke fails, when the API returns an error, then the UI keeps the
  token visible and explains the failure.
- [ ] Given workspace identity exists, when settings render, then workspace name,
  ID, and current user role are visible.
- [ ] Given an unauthorized or cross-workspace settings request, when it fails,
  then the UI shows not-found/permission copy without exposing another workspace.

**Tasks**

- [ ] Add token list and revoke API calls.
- [ ] Build token settings table/list with revoke action.
- [ ] Build workspace identity/settings panel.
- [ ] Add query invalidation after revoke.
- [ ] Add Vitest coverage for revoke success/failure states.

**Notes**

- Member management can remain read-only or hidden unless invite-by-email exists.
- Postponed until the MCP is more feature complete; revalidate the linked spec
  and UX before reopening because the current requirements may no longer apply.
