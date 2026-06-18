# 001 - PASETO Keyrings And Token Primitives

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#library-and-protocols)

**Summary** - Add the `pasetors` foundation for Proofplane's two token profiles:
`v4.public` API access and `v4.local` attachment grants. Keep protocol and key
handling behind small Proofplane-owned interfaces so later tickets do not
couple services to crate-specific types.

**Acceptance criteria**

- [x] Given valid configured keyrings, when the runtime starts, then one active API signing key and one active download encryption key are available with their verification/decryption rings.
- [x] Given malformed keys, duplicate or unknown active key IDs, or missing required keys, when configuration loads, then startup fails without exposing key material.
- [x] Given a token signed/encrypted for one Proofplane purpose, when it is verified under the other purpose, wrong key, unknown `kid`, or altered content, then verification fails closed.
- [x] Given existing Auth0 authentication and JWT download grants, when this foundation ships, then their runtime behavior is unchanged.

**Tasks**

- [x] Add and pin `pasetors` with the features required for v4 claims, public tokens, local tokens, and PASERK-compatible key parsing.
- [x] Add redacted configuration types for active keys and verification/decryption keyrings.
- [x] Implement Proofplane-owned API-token signer/verifier and download-grant encryptor/decryptor primitives with separate implicit assertions.
- [x] Add authenticated `kid` footers and safe untrusted-footer key selection.
- [x] Validate registered claims centrally and expose typed custom-claim parsing hooks.
- [x] Add unit tests for round trips, official failure classes, key rotation, purpose separation, and invalid startup configuration.

**Notes**

- `pasetors` custom claims are not validated automatically; each consuming
  profile must validate its complete custom payload.
