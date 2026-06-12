# Sandbox Product Launch Spec

## Goal

Let a founder discover Proofplane, create or resume a realistic SOC 2 sandbox,
connect their AI agent through MCP, and receive a useful compliance answer
without a sales conversation.

This epic adds the minimum browser surface needed for discovery, authentication,
sandbox provisioning, and MCP credential setup. Compliance work happens through
the customer's agent, not browser forms. The specific frontend framework should
be selected in ticket 001 after a short repository decision record; the contract
below is framework-independent.

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

The launch flow needs:

- human management support for actor creation/key issuance;
- sandbox create-or-resume endpoint;
- a safe browser session and CSRF strategy for management operations.
- MCP configuration snippets for supported agent clients;
- enough MCP read/write tools for the suggested first-run prompts.

The browser must not expose actor API keys after the one-time issuance response.
It does not provide MVP forms for controls, Evidence Requests, mappings, source
material, or packet generation.

## Agent-First Success Path

After provisioning, the setup page helps the owner issue an agent credential and
configure their MCP client. It then provides suggested prompts such as:

- "What do I have left to do for SOC 2 compliance?"
- "Show me the evidence requests that are due or missing evidence."
- "Create an access-review evidence request and map it to the relevant control."
- "Preview the auditor packet for my access-control evidence."

The first useful artifact is the agent's answer backed by Proofplane MCP tool
calls. Suggested prompts are instructional copy, not a Proofplane-hosted chat
interface. Tool results operate on real sandbox records.

## Funnel Events

Record:

- landing page viewed;
- sandbox started/resumed;
- agent credential issued;
- MCP setup instructions viewed;
- first successful MCP tool call;
- first MCP write;
- first packet preview through MCP;
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
- 2026-06-11: Replaced browser forms and dashboard-first onboarding with MCP
  credential setup and prompt-driven agent interaction.
