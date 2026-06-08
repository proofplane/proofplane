# 003 — Actor & API Key Management

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#database-changes-new-migration-v002)

**Summary** — Let a workspace owner/admin create workspace-scoped actors and issue API keys for them, completing the human → workspace → actor hierarchy. Gives actors a DB home and enables key rotation.

**Acceptance criteria**

- [ ] Given an owner/admin, when they create an actor, then it is workspace-scoped (`workspace_id`, `created_by_user_id`) and receives a SpiceDB `member` tuple.
- [ ] Given an actor with two live credentials, when either key is presented, then it authenticates; and when one is revoked, then the other still works.
- [ ] Given a presented key, when authenticating, then the credential is resolved by `key_id` scoped to the actor; given an unknown/revoked/expired key, then 401.
- [ ] Given a key issuance, when the response returns, then the raw key appears exactly once and is never persisted in plaintext, re-shown, or logged.
- [ ] Given a caller without `manage_actors`, when they manage actors/credentials, then 404; given an `actor_id` outside the path workspace, then it is rejected.
- [ ] Given a seeded system actor with a null `workspace_id`, when it authenticates, then it still succeeds.
- [ ] Given the `x-proofplane-*` contract, `ActorContext`, and data-plane routes, when this ships, then they are otherwise unchanged.

**Tasks**

- [ ] Migration: `actors` columns; drop unique-credential constraint + index `actor_id`.
- [ ] Change `ApiKeyAuthenticator::authenticate` to resolve by `key_id` (`api_credential_by_actor_and_key_id`).
- [ ] Add `manage_actors` to `.zed` + `WorkspaceAuthorizer`; deploy.
- [ ] Actor router (`authenticate_user` + `manage_actors`): create/list actors (member-tuple dual-write).
- [ ] Issue (raw key once) + revoke (idempotent) credential endpoints.
- [ ] Tests (multi-key, sibling survival, cross-actor reject, no secret leakage) + seed data.

**Notes**

- Evolves story 010: actors become workspace-owned; rotation allows >1 live key (issue-new-then-revoke-old). Detail in spec.
