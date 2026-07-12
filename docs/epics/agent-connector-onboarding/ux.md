# Agent Connector Onboarding UX

This document defines the customer-visible behavior for ticket 005. The core
rule is simple: users grant a named client access to Proofplane. Workspace,
role, resource, client identifiers, and OAuth scopes never appear in consent
or connection management.

## Consent

The consent page is server-rendered on the Proofplane API origin and uses the
Proofplane visual language.

- Heading: `Grant {client} access to Proofplane?`
- Supporting copy: access can be revoked later from Proofplane connection
  settings.
- Primary action: `Grant access`
- Secondary action: `Cancel`

The client display name is HTML-escaped. Rendering still requires a valid,
unconsumed authorization request, a verified upstream identity, and the user's
single workspace. Approval rechecks membership. Cancellation consumes the
request and returns `error=access_denied` with the original OAuth state.

Invalid, expired, replayed, or malformed requests show only:
`Connection could not be completed` and guidance to return to the client and
start again. They do not reveal client or account details.

## Guided Setup

The connection page begins with one copyable Proofplane MCP URL and two cards
labeled `Verified setup`.

### Claude Desktop

1. Open Customize → Connectors.
2. Select `+`, then `Add custom connector`.
3. Paste the Proofplane MCP URL, add it, and select `Connect`.
4. In the browser, select `Grant access`.

### ChatGPT Desktop

1. Open Settings → Plugins and select MCPs.
2. Select `Add server` and choose `Streamable HTTP`.
3. Name it Proofplane, paste the MCP URL, save, and select the circular-arrows
   refresh icon.
4. Select `Authenticate`; in the browser, select `Grant access`.

No setup path asks for a terminal command or configuration-file edit. Generic
client guidance remains ticket 008.

The page explains that access tokens expire after 24 hours. If a client asks
for authentication again, the user returns to that client's connector or MCP
server settings and selects `Connect` or `Authenticate`.

The first copyable prompt is:

> Review my SOC 2 readiness. Start by listing the highest-priority evidence
> gaps and the next action for each.

## Connection Rows

The connection list includes only the signed-in user's authorized and active
connections, newest authorization first.

Each row contains:

- client name;
- `Access granted` for an authorized connection or `Used` for an active one;
- authorization date and time;
- last-use date and time, or `Not yet`; and
- `Revoke`.

The labels describe Proofplane's recorded state, not whether the external
client is currently healthy. Empty, loading, and recoverable error states use
the same page shell. No row displays workspace, role, resource, scopes,
subjects, or client IDs.

## Revocation

`Revoke` expands an inline confirmation in the same row. While the request is
pending, both confirmation actions are disabled and the row stays visible.

On success, the row is removed and a status message explains that the user can
also remove Proofplane separately from the client's settings. On failure, the
row remains and the inline error states that the connection is still active.

Local revocation is immediately authoritative for MCP access. Proofplane does
not revoke the upstream Auth0 login grant and does not attempt client-side
disconnection.

## Protocol Boundary

OAuth scopes remain validated, stored, signed into access tokens, intersected
with the stored connection grant, and enforced for every MCP tool call. Their
absence from the product UI is intentional and does not mean that clients
receive unscoped access.
