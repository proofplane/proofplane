# Workspace Invitation Production Runbook

This runbook is the human-operated release gate for workspace invitations. Run
it independently in every environment. Do not paste access tokens, invitation
links, PASETO keys, provider keys, or DNS credentials into tickets, chat, logs,
screenshots, or the evidence record.

## Ownership And Evidence

Before changing production, record named people or on-call rotations for these
roles in the deployment inventory:

| Responsibility | Required owner |
| --- | --- |
| Auth0 tenant, Action, and access-token claims | Identity owner |
| Resend account, restricted API key, and sender identity | Messaging owner |
| SPF, DKIM, and DMARC records | DNS owner |
| Runtime configuration and secret delivery | Deployment owner |
| Invitation email subject, body, and support copy | Product/content owner |
| Delivery monitoring and incident review | Operations owner |

Create a release evidence record containing the environment, date, release
revision, non-secret Auth0 tenant/application identifiers, sender address and
subdomain, DNS/provider verification status, smoke-test invitation ID, and the
reviewers for each section below. Record pass/fail and timestamps, never secret
values, email addresses, tokens, URLs containing tokens, or provider bodies.

## Auth0 Claims Action

1. In the management-plane Auth0 tenant, add a post-login Action that sets the
   three access-token claims documented in
   [Auth0 management identity claims](./auth0-management-identity.md). Apply it
   only to the existing management-plane login flow; do not alter the auditor
   or MCP clients, their audiences, or Auth0 Organizations.
2. Deploy and bind the Action through the tenant's reviewed change process.
3. Authenticate a test user whose mailbox is verified. Decode a short-lived
   Proofplane API access token locally and confirm its existing issuer,
   audience, subject, and permissions are unchanged and that it contains:
   `https://proofplane.com/email` and `https://proofplane.com/name` as the
   expected strings and
   `https://proofplane.com/email_verified` as boolean `true`.
4. Repeat with an unverified test identity and confirm Proofplane does not
   treat it as verified invitation authority. Do not retain either token.
5. Confirm an unrelated management login and the auditor and MCP login paths
   still use their previous clients and audiences.

Rollback is to unbind the Action version from the management login flow and
redeploy the previously recorded flow version. This disables invitation
acceptance that needs verified identity; it must not disable ordinary login.

## Resend, DNS, And Sender

1. Create a dedicated invitation sending subdomain. Publish the exact SPF and
   DKIM records supplied by Resend and an explicit DMARC record whose policy and
   report destinations are approved by the DNS and security owners.
2. Wait for both DNS inspection and Resend to report SPF and DKIM verified.
   Confirm DMARC with an independent DNS lookup. Record statuses and timestamps,
   not provider verification tokens or account credentials.
3. Configure a sender identity on that subdomain and have the product/content
   owner review the invitation subject, body, expiry language, and support copy.
4. Create a Resend API key restricted to sending access and, where the provider
   supports it, to this sending domain. Put it directly into the deployment
   secret manager; never first save it in source control or a local config file.
5. Render the production configuration through the normal secret path. Its
   non-secret shape is:

   ```yaml
   workspace_invitations:
     landing_portal_base_url: "https://<portal-host>"
     active_key_id: "<current-key-id>"
     keys:
       - id: "<current-key-id>"
         secret: "<secret-managed PASERK>"
   mail:
     adapter: "resend"
     api_key: "<secret-managed Resend key>"
     from: "Proofplane <invitations@<sending-subdomain>>"
   ```

6. Start both the API and worker with the rendered configuration. Confirm
   startup succeeds and application/debug output contains none of the secrets.

If delivery must be stopped, disable or revoke the Resend key and stop the
worker while leaving the API available. Pending invitations remain usable by
copying their links. Restore delivery only after the incident owner approves a
new restricted key and the worker configuration has been redeployed.

## Secret And Key Rotation

- Rotate the Resend key by creating a second restricted key, deploying it,
  proving one delivery, then revoking the old key. Roll back by redeploying the
  still-valid old key before it is revoked.
- Rotate invitation PASETO authority by adding a new key alongside the old one,
  changing `active_key_id`, and deploying the API and worker together. Keep the
  old key until every invitation issued with it has expired or been revoked;
  removing it early invalidates otherwise active links. Roll back by restoring
  the previous active key while both keys remain configured.
- Rotate or roll back the Auth0 Action by deploying versioned Action code and
  recording the previously bound version before changing the flow.

After any rotation, inspect logs and the evidence record for accidental secret
disclosure and revoke the affected authority immediately if one is found.

## Inviter-To-Invitee Smoke Journey

Use a production-like environment and a dedicated inviter and invitee. The
invitee must have a verified Auth0 mailbox and no existing workspace. Preserve
only non-secret IDs in the evidence record.

1. As an owner or administrator, open People, create an administrator
   invitation, and confirm it appears once as pending with an expiry and
   delivery state.
2. Confirm the worker sends one message through Resend and the recipient gets
   it. Verify provider and application diagnostics do not expose the recipient
   address, invitation URL, token, or provider response body.
3. Open the email link in a fresh browser context. Confirm the fragment token is
   removed from the address bar before navigation or Auth0 redirection, sign in
   with the matching verified mailbox, accept, and verify exactly one
   administrator membership is created. Replaying the link must not create a
   second membership.
4. Create another invitation for a second dedicated invitee. From People,
   explicitly copy its current link and deliver it through the approved manual
   channel. Complete matching-account confirmation and verify exactly one
   administrator membership.
5. Exercise send-again and confirm the earlier generation stops working. Revoke
   a separate pending invitation and confirm its link no longer works.
6. Confirm existing workspace creation, login, agent connections, auditor
   invitations, member removal, and an unrelated Auth0 client still operate.

## Delivery Diagnosis

- **Queued:** confirm the worker is running and consuming its outbox; inspect
  bounded retry/error class logs using the invitation ID and generation only.
- **Failed:** inspect Resend status class, key restriction, sender verification,
  domain status, suppression/bounce state, and worker retry exhaustion. Never
  copy a provider response body into shared diagnostics.
- **Delivered but absent:** verify recipient filtering, Resend event state,
  SPF/DKIM alignment, DMARC reports, and sending reputation with the messaging
  and DNS owners. The manager can use the current copyable link meanwhile.
- **Auth0 mismatch:** verify the token audience and namespaced claim types, then
  the invited mailbox match. Do not use ordinary profile email or `login_hint`
  as proof of mailbox control.

The deployment owner signs the release gate only after every section has a
named reviewer and the two smoke paths pass. Any failure leaves the ticket open
and records the non-secret symptom, owner, and rollback decision.
