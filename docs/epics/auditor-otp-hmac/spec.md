# Auditor OTP HMAC Spec

## Goal

Replace plain SHA-256 digests of six-digit auditor OTPs with HMAC-SHA-256 so a
database-only attacker cannot test the one million possible codes without also
obtaining a server-held key. Preserve the existing ten-minute expiry,
five-attempt verification limit, three-send rate limit, digest-only persistence,
and Resend delivery behavior.

## Existing Baseline And Threat Model

Auditor OTP issuance generates a six-digit code, stores
`SHA256(code)` in `auditor_access_otps`, and sends the plaintext code through
the configured mail adapter. Verification hashes the submitted code and
compares it with the newest eligible row. Session tokens use the same SHA-256
helper but contain 256 bits of randomness.

Plain SHA-256 is collision-resistant but does not protect a one-million-value
OTP space from offline enumeration. This epic protects against compromise of
Postgres without simultaneous compromise of application configuration. It does
not protect a host where both the database and runtime secrets are available.

Session-token digests remain SHA-256. Their input entropy already makes offline
enumeration infeasible, and changing their representation would invalidate
active sessions without improving the relevant threat boundary.

## Key Configuration And Rotation

Add a dedicated application configuration section:

```yaml
auditor_access:
  otp_hmac:
    active_key_id: "otp-hmac-001"
    keys:
      - id: "otp-hmac-001"
        secret: "<base64url-no-pad encoding of exactly 32 random bytes>"
```

The keyring follows the existing PASETO keyring pattern:

- key IDs are non-empty and unique;
- `active_key_id` must identify exactly one configured key;
- secrets are base64url without padding and decode to exactly 32 bytes;
- decoded key material and configuration debug output remain redacted;
- missing, malformed, duplicate, or unresolved configuration fails startup;
- issuance uses only the active key;
- verification selects the key recorded on the OTP row;
- unknown or removed key IDs fail closed without a panic or fallback.

Rotation adds a new key, makes it active, deploys that configuration to all API
instances, and retains the old key until at least ten minutes after the final
old instance stops issuing OTPs. No persisted OTP is re-keyed. Removing an old
key early intentionally invalidates OTPs that reference it.

Local committed configuration contains a non-production development key.
Production key material belongs only in the secret-managed YAML referenced by
`PROOFPLANE_CONFIG`.

## Digest Contract

Define one auditor-OTP keyed-digest component shared by issuance and
verification. For version 1, the authenticated message is the byte
concatenation:

```text
"proofplane:auditor-otp:v1\0"
+ grant UUID canonical 16 bytes
+ six ASCII code bytes
```

The output is the 32-byte HMAC-SHA-256 tag. The persisted key ID identifies the
key but is not part of the authenticated message. The domain prefix and grant
binding prevent reuse of this primitive across purposes or grants.

Verification uses the `hmac` crate's constant-time tag verification API rather
than ordinary slice equality. The component accepts only six ASCII digits; an
invalid shape is rejected before computing a tag. Errors and debug output must
not include the OTP, key, computed tag, or candidate input.

Use a distinct function or type for session-token SHA-256 so future changes
cannot accidentally route low-entropy OTPs through the unkeyed helper.

## Persistence And Runtime Flow

Add `digest_key_id TEXT NOT NULL` to `auditor_access_otps` with a non-blank
check. Keep `code_digest BYTEA` and its 32-byte length constraint; the column
now stores the HMAC tag.

Issuance:

1. Generate the OTP and row ID.
2. Compute the tag from the active key, grant ID, and code.
3. Store the tag and active key ID before delivery.
4. Send the plaintext code through the existing mail adapter.
5. Delete the new row on final mail failure, preserving current cleanup and
   rate-limit behavior.

Verification locks the newest eligible OTP, resolves its recorded key, and
verifies the submitted code in constant time. Correct codes retain the current
atomic consume-and-session-create behavior. Wrong codes retain the current
failed-attempt increment. Missing keys or malformed persisted key IDs fail
closed as an internal configuration/data error and never consume the OTP.

The repository currently maintains a consolidated `V001` initial schema.
Update that schema directly and reset/reseed local databases. There is no
legacy `SHA256(code)` verification fallback and no mixed-format row: existing
development OTP rows are disposable because they expire after ten minutes. If
an environment has already applied `V001` outside disposable development,
create an additive migration that invalidates existing OTP rows before making
`digest_key_id` required; never rewrite an applied production migration.

## Security And Observability

Persist only the HMAC tag and non-secret key ID. Never persist or log the
plaintext code, HMAC key, Authorization header, Resend payload, or computed
candidate tag. Logs may include a stable coarse error category and key ID for
an unknown-key configuration failure, but not key material.

No email copy, browser behavior, public endpoint, HTTP status, cookie,
invitation-token, rate-limit, or session-lifetime contract changes. Resend
idempotency continues to use the OTP row ID and is independent of the digest
key ID.

## Test Contract

- Use fixed RFC-compatible HMAC test vectors to prove deterministic output and
  verification.
- Prove different grants, codes, and keys produce different tags.
- Prove malformed codes and unknown keys fail closed.
- Prove configuration validation and redaction for every keyring invariant.
- With concrete Postgres, prove rows contain a 32-byte tag and key ID but not
  the plaintext code.
- Prove rotation: old in-flight OTPs verify with their recorded key while new
  OTPs use the active key.
- Preserve wrong-code attempt counting, successful atomic consumption, mail
  failure cleanup, rate limiting, expiry, and session-token digest behavior.

## Revisions

- 2026-07-24: Initial spec. Chose grant-bound HMAC-SHA-256, persisted key IDs,
  constant-time verification, no legacy SHA fallback, and unchanged SHA-256
  session-token digests.
