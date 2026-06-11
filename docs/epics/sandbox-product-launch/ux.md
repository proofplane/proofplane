# Sandbox Product Launch UX

## Core Principle

The first session must create real Proofplane records and reach a useful artifact
within five minutes. Avoid empty dashboards, fake tours, and sales-gated calls to
action.

## Entry Flow

1. Homepage primary CTA: **Start SOC 2 Sandbox**.
2. Auth0 login or account creation with minimum required identity fields.
3. Automatic create-or-resume sandbox provisioning.
4. Redirect to the sandbox overview, not a setup wizard splash screen.

Provisioning shows progress and can be safely retried. A partial failure offers
retry with the same workspace rather than creating another one.

## First-Run Flow

The overview shows starter controls, one mapped Evidence Request, and a clear
three-step checklist:

1. Edit or create a control.
2. Create an Evidence Request and map it.
3. Open the auditor packet preview.

Each step links directly to the relevant form. Completion is derived from saved
records, so refresh and resume preserve progress.

## Packet Preview

Show:

- selected control and framework requirements;
- mapped Evidence Requests;
- latest evidence/freshness state or an explicit missing-evidence gap;
- source-material provenance;
- attachment inventory without exposing unusable download links;
- actor and timestamps from record provenance.

The preview should remain useful before a file is uploaded by clearly showing
what is missing.

## API And Agent Moment

After the first packet preview, show the workspace actor and MCP/API setup path.
Reveal a newly issued raw key only once, with revoke/rotate guidance. Do not put
credentials in URLs, analytics, or browser persistence beyond the immediate
display needed by the user.

## Public Pages

Use plain, answer-first copy. Pricing is reachable from every public page.
Comparison and guide pages lead to the same sandbox CTA and do not require
signup to read.

## Responsive And Accessible Behavior

The primary flow must work at narrow mobile and desktop widths, use keyboard
navigation, visible focus, semantic form errors, and sufficient contrast.
Loading, empty, error, and retry states are part of each screen's acceptance
criteria, not deferred polish.
