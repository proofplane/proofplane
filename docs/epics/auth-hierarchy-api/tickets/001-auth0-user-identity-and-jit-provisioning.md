# 001 — Auth0 User Identity & JIT Provisioning

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#two-auth-middleware-paths)

**Summary** — Add the human-identity auth plane: validate Auth0 access tokens, provision a `users` record on first login, and expose a `UserContext` to handlers. Foundation every management route builds on.

**Acceptance criteria**

- [x] Given a valid Auth0 access token, when a request hits a protected route, then it authenticates and `UserContext` is available to the handler.
- [x] Given a missing, expired, wrong-`aud`/`iss`, tampered, or non-RS256 token, when a request is made, then it returns 401.
- [x] Given a first-seen `auth0_sub` (including concurrent first requests), when the token is verified, then exactly one `users` row is created and reused thereafter.
- [x] Given a token with no `email`/`name` claims, when the user authenticates, then provisioning and authorization succeed on `auth0_sub` alone.
- [x] Given an authenticated caller, when they call `GET /me`, then their user is returned; given no/invalid token, then 401.
- [x] Given an authenticated request, when it is logged, then `user_id` is present and the token/`Authorization` header is absent.
- [x] Given an existing API-key data-plane request, when this ships, then it authenticates exactly as before.

**Tasks**

- [x] `users` migration + `User`/`UserId` + repo upsert/get by `auth0_sub`.
- [x] Typed `auth0` config (domain, audience, JWKS url) with validation.
- [x] `TokenVerifier` adapter wrapping `jwtk` (RS256 pinned, `iss`/`aud`/`exp` checks, JWKS cache).
- [x] `UserContext` + `authenticate_user` middleware (verify → JIT provision → attach → 401).
- [x] `GET /me` route, mounted in `src/app.rs`.
- [x] Tests (local-keypair token minting; no live Auth0) + seed data.

**Notes**

- Crate choice: `jwtk` preferred, `jsonwebtoken` + JWKS cache fallback; verify crate health at build time. Rationale in spec.
- Validate **access** tokens (audience = the API), not ID tokens — hence optional profile claims.
- Built on `jwtk` 0.5 (default `aws-lc` + `remote-jwks`, both already in the tree). The `aws-lc` backend is RSA verify-only, so unit-test token minting uses `jwtk`'s `openssl` backend as a **dev-dependency** — production stays free of `openssl-sys`.
- Migration `V002__users.sql` adds only the `users` table; `workspace_memberships`, actor columns, and the `api_credentials` index changes from the spec's `V002` belong to tickets 002/003.
