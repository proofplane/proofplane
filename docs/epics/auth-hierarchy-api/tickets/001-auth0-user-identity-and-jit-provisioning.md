# 001 — Auth0 User Identity & JIT Provisioning

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#two-auth-middleware-paths)

**Summary** — Add the human-identity auth plane: validate Auth0 access tokens, provision a `users` record on first login, and expose a `UserContext` to handlers. Foundation every management route builds on.

**Acceptance criteria**

- [ ] Given a valid Auth0 access token, when a request hits a protected route, then it authenticates and `UserContext` is available to the handler.
- [ ] Given a missing, expired, wrong-`aud`/`iss`, tampered, or non-RS256 token, when a request is made, then it returns 401.
- [ ] Given a first-seen `auth0_sub` (including concurrent first requests), when the token is verified, then exactly one `users` row is created and reused thereafter.
- [ ] Given a token with no `email`/`name` claims, when the user authenticates, then provisioning and authorization succeed on `auth0_sub` alone.
- [ ] Given an authenticated caller, when they call `GET /me`, then their user is returned; given no/invalid token, then 401.
- [ ] Given an authenticated request, when it is logged, then `user_id` is present and the token/`Authorization` header is absent.
- [ ] Given an existing API-key data-plane request, when this ships, then it authenticates exactly as before.

**Tasks**

- [ ] `users` migration + `User`/`UserId` + repo upsert/get by `auth0_sub`.
- [ ] Typed `auth0` config (domain, audience, JWKS url) with validation.
- [ ] `TokenVerifier` adapter wrapping `jwtk` (RS256 pinned, `iss`/`aud`/`exp` checks, JWKS cache).
- [ ] `UserContext` + `authenticate_user` middleware (verify → JIT provision → attach → 401).
- [ ] `GET /me` route, mounted in `src/app.rs`.
- [ ] Tests (local-keypair token minting; no live Auth0) + seed data.

**Notes**

- Crate choice: `jwtk` preferred, `jsonwebtoken` + JWKS cache fallback; verify crate health at build time. Rationale in spec.
- Validate **access** tokens (audience = the API), not ID tokens — hence optional profile claims.
