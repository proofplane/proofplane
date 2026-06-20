# 005 - PASETO Attachment Download Grants

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#attachment-download-grants)

**Summary** - Replace readable HS256 JWT attachment grants with encrypted and
authenticated `v4.local` PASETO grants. Keep the five-minute bearer URL,
stateless redemption, and all current attachment and object-integrity checks.

**Acceptance criteria**

- [x] Given an authorized API-token caller and an eligible attachment, when a grant is issued, then the returned URL contains a five-minute `v4.local` token whose identifiers are not readable from the token payload.
- [x] Given a valid grant, when it is redeemed before expiry, then the attachment is reloaded, remains eligible, passes metadata checks, and streams with the existing cache and referrer protections.
- [x] Given a tampered, expired, wrong-key, wrong-purpose, malformed, or unsupported-version grant, when it is redeemed, then the endpoint returns the same not-found response without leaking failure details.
- [x] Given a legacy JWT grant, when it is redeemed after this migration, then it is rejected through the same not-found response as any invalid token.
- [x] Given an attachment that becomes ineligible after grant issuance, when a valid PASETO grant is redeemed, then download remains denied as it is today.

**Tasks**

- [x] Define and validate the versioned `v4.local` download claims with user and API-token attribution.
- [x] Issue PASETO grants with the dedicated local keyring, footer `kid`, purpose assertion, and unchanged five-minute TTL.
- [x] Redeem only PASETO grants and remove the legacy HS256 JWT helper and signing-secret configuration in the same change.
- [x] Preserve URL redaction, HTTPS requirements, `private, no-store`, `no-referrer`, and generic not-found errors.
- [x] Add unit and integration tests for encryption, rotation, rejection cases, JWT rejection, and unchanged attachment eligibility/integrity behavior.

**Notes**

- Revised with the 2026-06-17 spec update: the undeployed service has no
  JWT/PASETO compatibility phase.
- The 2026-06-19 opaque user-token pivot does not change this ticket's
  `v4.local` grant profile; issuance continues to record the authenticated user
  and API-token IDs supplied by `ApiTokenContext`.
