# Keep workspace invitation authority in Proofplane

Proofplane owns workspace invitations and memberships while Auth0 authenticates
the invited person. A workspace invitation is persisted in Proofplane, delivered
through a replaceable mail adapter, and accepted only after Auth0 supplies a
matching verified email. We chose this over Auth0 Organizations because mapping
every workspace to an Auth0 Organization would make tenant capacity, membership
authority, and core authorization behavior dependent on Auth0 plan limits and
Management API availability.

## Consequences

- The workspace aggregate remains authoritative for memberships and the
  last-owner invariant.
- Auth0 proves identity and verified mailbox ownership but does not create,
  remove, or assign Proofplane memberships.
- Invitation issuance, expiry, revocation, resend, acceptance, replay, and
  concurrency behavior are explicit Proofplane domain and application behavior.
- Proofplane owns invitation delivery through an external mail adapter and must
  operate its retries, secrets, observability, and sender-domain configuration.
- Invite links remain available for manual sharing even when email delivery is
  configured.
- Auth0 Organizations may be reconsidered only as a deliberate tenant-model
  migration, not as an implementation detail of workspace invitations.
