# Agent connection Redirect Action

Deploy `agent-connection-redirect.js` as an Auth0 Post Login Action and bind it
after authentication. It intentionally has no npm dependencies.

Configure these Action secrets:

- `PROOFPLANE_API_BASE_URL`: canonical Proofplane API origin, without a path.
- `PROOFPLANE_MCP_RESOURCE`: exact Auth0 MCP resource identifier.
- `PROOFPLANE_ALLOWED_CLIENT_IDS`: comma-separated third-party client IDs.
- `PROOFPLANE_ACTION_SHARED_SECRET`: the same 32-byte-or-longer secret as
  `auth0.action.shared_secret` in Proofplane.

The Action must run for both initial execution and redirect continuation.
Configure the Proofplane API URL as an allowed Action redirect destination.
Auth0 appends and validates its own opaque redirect `state`; Proofplane echoes
that value unchanged to the tenant issuer’s `/continue` endpoint.

After deployment, verify visible approval and denial, `prompt=none` returning
`interaction_required` when no reusable connection exists, continuation
single-use behavior, and active-connection reuse in the development tenant.

