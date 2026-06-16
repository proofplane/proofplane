# 003 — Actor & API Key Management

**Status:** Done · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#database-changes-new-migration-v002)

**Summary** — Let a workspace owner/admin create workspace-scoped actors and issue API keys for them, completing the human → workspace → actor hierarchy. Actors are bound to exactly one workspace (`actors.workspace_id` NOT NULL) and carry explicit per-actor permission grants. **SpiceDB was removed**: data-plane authorization is now answered from Postgres.

**Acceptance criteria**

- [x] Given an owner/admin, when they create an actor, then it is workspace-scoped (`workspace_id`, `created_by_user_id`) and is created with the explicit permission grants supplied in the request.
- [x] Given an actor with two live credentials, when either key is presented, then it authenticates; and when one is revoked, then the other still works.
- [x] Given a presented key, when authenticating, then the credential is resolved by `key_id` scoped to the actor; given an unknown/revoked/expired key, then 401.
- [x] Given a key issuance, when the response returns, then the raw key appears exactly once and is never persisted in plaintext, re-shown, or logged.
- [x] Given a caller who is not a workspace owner/admin, when they manage
  actors/credentials, then 404; given an `actor_id` outside the path workspace,
  then it is rejected.
- [x] Given a data-plane request, when the actor does not belong to the path
  workspace **or** lacks the permission for the route+method, then 404 (no
  existence leak); when it belongs and holds the permission, then it proceeds.
- [x] Given the `x-proofplane-*` contract, `ActorContext`, and data-plane routes, when this ships, then the header contract and route surface are otherwise unchanged.

**Tasks**

- [x] Migration: `actors.workspace_id` (NOT NULL, backfilled to a system workspace) + `created_by_user_id`; drop the unique-credential constraint and re-add a non-unique `actor_id` index; add the `actor_permissions` table and backfill existing actors with all six permissions.
- [x] Change `ApiKeyAuthenticator::authenticate` to resolve by `key_id` (`actor_credential_by_key_id`), returning the actor's home workspace + permission set on `ActorContext`.
- [x] Remove SpiceDB (client, proto build, config, infra, schema) and replace the data-plane guards' permission checks with the Postgres-sourced `ActorContext.permissions`; enforce the workspace binding in `authorize_workspace_route`.
- [x] Authorize actor management from Postgres owner/admin membership (`WorkspaceMemberPolicy::can_manage_actors`).
- [x] Actor router (`authenticate_user`): create/list actors.
- [x] Issue (raw key once) + revoke (idempotent) credential endpoints.
- [x] Tests (multi-key, sibling survival, cross-actor reject, no secret leakage, per-permission enforcement, workspace binding) + seed data.

**Notes**

- Actors become workspace-owned; rotation allows more than one live key
  (issue-new-then-revoke-old).
- The original spec authorized the actor data plane through SpiceDB with an
  outbox dual-write. That was dropped in favor of a single Postgres source of
  truth: an actor belongs to exactly one workspace and holds an explicit set of
  the six data-plane permissions. See the spec's revised authorization section.
