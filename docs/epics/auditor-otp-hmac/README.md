# Auditor OTP HMAC Epic

Protect auditor one-time codes against offline recovery from a database-only
compromise by replacing their plain SHA-256 digests with domain-separated,
versioned HMAC-SHA-256 values. The core principle is that persisted OTP state
must be useless for testing six-digit candidates without a separately managed
application secret.

Full rationale, key-rotation rules, persistence contracts, and cutover decisions
live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [OTP HMAC Keyring](./tickets/001-otp-hmac-keyring.md) | Todo | Add validated key configuration and a constant-time keyed digest component. |
| 002. [Auditor OTP HMAC Cutover](./tickets/002-auditor-otp-hmac-cutover.md) | Todo | Persist key IDs and use keyed digests throughout OTP issue and verification. |

## Sequencing

- **001** is foundational because issuance and verification need one validated,
  rotation-aware keyed-digest API.
- **002** depends on 001 and performs the schema and service cutover.
- The tickets are sequential; there is no safe runtime cutover before the
  keyring and verification behavior are available.
