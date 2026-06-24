# 004 - Scoped Token Creation Flow

**Status:** Todo · **Depends on:** 003, paseto-token-migration/002, paseto-token-migration/006 · **Spec:** [spec.md](../spec.md#token-permission-model)

**Summary** - Build the token creation flow with job-based permission presets,
granular permission visibility, and a one-time raw-token success state.

**Acceptance criteria**

- [ ] Given a workspace owner/admin, when they choose a permission preset, then
  the exact granular permissions are shown before token creation.
- [ ] Given a custom permission selection, when no permissions are selected, then
  token creation is blocked with a clear validation message.
- [ ] Given a successful token response, when the success screen renders, then
  the raw token is shown once with copy actions and a save acknowledgement.
- [ ] Given the user leaves the success screen, when they return to tokens later,
  then the raw token is not shown again.
- [ ] Given a token creation API error, when submission fails, then no local raw
  token state is retained.

**Tasks**

- [ ] Add typed token create DTOs and mutation.
- [ ] Implement permission preset mapping and custom selection.
- [ ] Build one-time token success panel with copy token/env/MCP config actions.
- [ ] Add save acknowledgement gate before continuing.
- [ ] Add Vitest coverage for preset mapping, validation, and one-time token
  state.

**Notes**

- The UI should not persist the raw token outside the in-memory mutation result
  and visible success state.
