# Sandbox Product Launch Spec

## Goal

Let a founder discover Proofplane, create or resume a realistic SOC 2 sandbox,
and reach an auditor packet preview without a sales conversation.

This epic adds a browser product surface to the current Rust backend. The
specific frontend framework should be selected in ticket 001 after a short
repository decision record; the contract below is framework-independent.

## Account And Sandbox Model

Auth0 remains the human identity provider. After login, a user may create or
resume one sandbox workspace. Add workspace mode `sandbox` or `standard` and a
stable sandbox ownership lookup so repeated CTA completion is idempotent.

Provisioning creates in one recoverable workflow:

- workspace and owner membership;
- starter SOC 2 controls and framework mappings;
- at least one mapped Evidence Request;
- one workspace-scoped AI-agent actor and initial credential display flow;
- realistic source material and packet-preview data where dependencies permit.

Provisioning must not clone the global seed workspace or grant access to another
customer's records.

## Product API Gaps

The existing data API can create Evidence Requests and controls, but the launch
flow also needs:

- human management support for actor creation/key issuance;
- sandbox create-or-resume endpoint;
- control create/replace UI contract;
- packet preview endpoint;
- a safe browser session and CSRF strategy for management operations.

The browser must not expose actor API keys after the one-time issuance response.

## Funnel Events

Record:

- landing page viewed;
- sandbox started/resumed;
- first control created or edited;
- first Evidence Request created;
- first mapping created;
- packet preview viewed;
- paid conversion, only when billing exists.

Product analytics events are not compliance audit events. They use a separate
adapter and must not contain control descriptions, evidence content, API keys,
or attachment metadata.

## Public Content

Launch pages:

- homepage;
- pricing;
- SOC 2 for startups;
- Vanta alternative for startups;
- Drata alternative for startups;
- SOC 2 evidence checklist;
- API/MCP documentation entry point.

Serve `robots.txt`, `sitemap.xml`, and `llms.txt`; add canonical metadata and
appropriate organization, software, product, article, breadcrumb, and FAQ
structured data. Comparison pages show a visible last-updated date.

## Security And Limits

Sandbox tenant isolation is identical to standard workspaces. Any reduced
retention, quotas, or disabled capabilities are explicit server-side policy, not
client-only controls. Billing and conversion are optional for launch; public
pricing is not.

## Revisions

- 2026-06-11: Separated reusable packet functionality into Trusted Compliance
  Reads and retained browser onboarding, marketing, analytics, and sandbox
  provisioning in this epic.
