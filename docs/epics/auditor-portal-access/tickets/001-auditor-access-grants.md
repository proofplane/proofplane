# 001 - Auditor Access Grants

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#auditor-access-grants)

**Summary** - Add the durable permission record behind each auditor invite so a
workspace can grant, list, expire, and revoke email-bound auditor access without
creating workspace membership or API tokens.

**Acceptance criteria**

- [ ] Given a token with `manage_auditor_access`, when it creates a grant for an
  email, then Proofplane stores only a digest of the invite secret.
- [ ] Given a revoked, expired, missing, or cross-workspace grant, when it is
  loaded for use, then access is rejected without leaking workspace existence.
- [ ] Given an ordinary compliance read token, when it attempts to create an
  auditor grant, then the request is concealed or denied.
- [ ] Given existing evidence/control reads, when this ships, then their
  authorization behavior is unchanged.

**Tasks**

- [ ] Add the `manage_auditor_access` workspace permission.
- [ ] Add grant schema, domain types, and repository methods.
- [ ] Add service create/list/revoke behavior with invite secret digesting.
- [ ] Emit grant create/revoke audit logs without raw secrets.
- [ ] Add integration tests for scope, expiry, revocation, and secret handling.
