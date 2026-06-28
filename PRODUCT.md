# Product

## Register

product

## Users

Proofplane is for founders, CTOs, engineering leaders, and operations leads at
5-50 person B2B SaaS and AI startups. They are usually responding to a customer
security request, starting SOC 2 readiness, or trying to turn scattered
compliance work into something reviewable without buying a heavy enterprise GRC
suite.

The user is capable and technical, but not usually a dedicated compliance
operator. They want to unblock revenue, understand what evidence is missing, and
let trusted agents do useful work without losing auditability or control.

Secondary users include startup admins who manage workspace membership and
scoped credentials, and auditors or advisors who consume exported evidence
packets rather than living inside Proofplane.

## Product Purpose

Proofplane is SOC 2 compliance infrastructure for AI-native startups. It gives
startups a self-serve workspace where humans manage the account and scoped
programmatic actors do data-plane work through APIs and MCP.

The product exists to make compliance data structured, permissioned, and
auditable enough for customer-owned agents to inspect evidence, create
submissions, map controls, and prepare auditor-ready packets. Success means a
founder can create a workspace, issue a scoped token, connect an agent, and see a
real SOC 2 evidence workflow take shape within minutes.

The first-run experience should produce a real artifact: a starter workspace,
starter controls, a scoped API token, MCP setup guidance, suggested prompts, and
a visible auditor packet preview with gaps and provenance.

## Landing Page Direction

The landing page is the brand surface and may use a more cinematic presentation
mode than the authenticated product UI. Its reference is the Perplexity Computer
landing page: a cinematic, long-scrolling product narrative with full-viewport
sections, sticky calls to action, immersive product visuals, short copy, and
scroll-driven progression. The product and landing page should share the same
dark green-black color system; the landing page earns more drama through scale,
spacing, and scroll behavior rather than a separate palette.

Proofplane should adapt that pattern to compliance. The landing page should make
compliance tasks feel simpler by showing a sequence of reduced actions:

- create the workspace;
- pick the token's job;
- connect agent access when MCP is ready;
- see evidence gaps;
- prepare an auditor packet.

The visitor should not feel like they are reading a compliance brochure. They
should feel the product turning a vague SOC 2 burden into visible steps.

## Brand Personality

The brand should feel precise, calm, and operational.

Proofplane should speak like a senior security-minded engineer explaining a
system clearly: direct, specific, low drama, and allergic to procurement theater.
It should create confidence without sounding like an enterprise compliance
platform or a generic AI wrapper.

The emotional target is controlled momentum. Users should feel that SOC 2 is
serious, but that the system has made the next step legible.

## Anti-references

Proofplane should not look or sound like:

- A cheaper clone of Vanta, Drata, or any broad enterprise GRC suite.
- A marketing site that forces "Book a Demo" before product access.
- A generic AI SaaS page with vague automation claims and decorative prompt
  boxes.
- A dense compliance spreadsheet with no guided first-run path.
- A chart-led dashboard that hides the next compliance task.
- A security product that uses fear, panic, or breach imagery to sell urgency.
- A developer tool that hides permissions behind unclear "full access" tokens.

For the landing page specifically, do not reject cinematic presentation, scroll
effects, sticky calls to action, or staged product visuals just because the app
UI is calmer. Use those moves when they simplify the story. For authenticated
screens, keep the same dark operational color scheme but reduce the drama:
smaller type, denser layouts, and direct task surfaces.

## Design Principles

1. Show the operating model early. Humans manage workspaces and credentials;
   actors and agents use scoped credentials to work with compliance data.

2. Turn setup into proof. The first five minutes should end with a workspace,
   starter controls, a token, MCP setup guidance, and an inspectable evidence
   packet preview.

3. Make permissions understandable. Token creation should use job-based
   presets, show the granular grants underneath, and make revocation obvious.

4. Prefer artifacts over explanations. Evidence requests, latest submissions,
   control mappings, provenance, and packet gaps should carry more weight than
   marketing copy.

5. Be honest about readiness. If MCP or packet export work is not production
   ready yet, label it clearly as setup preview, coming soon, or branch work
   instead of implying completion.

6. Let the landing page unfold through scroll. Each section should carry one
   simplification, one visual artifact, and one clear next action.

## Accessibility & Inclusion

Target WCAG 2.2 AA for all public and authenticated UI. The product should work
with keyboard navigation, visible focus states, screen readers, reduced motion,
and common color-vision deficiencies.

Compliance work often happens under time pressure, so states must be explicit:
token shown once, token not saved, no workspace, no permission, MCP unavailable,
export pending, and evidence missing should each have clear recovery paths.

Use plain language for compliance and authorization concepts. Technical terms
such as workspace, actor, token, permission, MCP, evidence request, and auditor
packet are acceptable, but each screen should make the next action clear without
requiring prior product knowledge.
