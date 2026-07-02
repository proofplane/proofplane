# 001 - Auditor Access Grants

**Status:** Doing · **Depends on:** none · **Spec:** [spec.md](../spec.md#auditor-access-grants)

**Summary** - Add the durable permission record behind each auditor invite so a
workspace can grant, list, expire, and revoke email-bound auditor access without
creating workspace membership or API tokens.

**Acceptance criteria**

- [x] Given a token with `manage_auditor_access`, when it creates a grant for an
  email, then Proofplane stores only a digest of the invite secret.
- [x] Given a revoked, expired, missing, or cross-workspace grant, when it is
  loaded for use, then access is rejected without leaking workspace existence.
- [x] Given an ordinary compliance read token, when it attempts to create an
  auditor grant, then the request is concealed or denied.
- [x] Given existing evidence/control reads, when this ships, then their
  authorization behavior is unchanged.

**Tasks**

- [x] Add the `manage_auditor_access` workspace permission.
- [x] Add grant schema, domain types, and repository methods.
- [x] Add service create/list/revoke behavior with invite secret digesting.
- [ ] Emit grant create/revoke audit logs without raw secrets.
- [x] Add integration tests for scope, expiry, revocation, and secret handling.

**Notes**

- Audit emission remains for the MCP caller surface in ticket 002; services return
  non-secret grant metadata for those logs.
