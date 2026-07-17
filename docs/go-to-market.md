# Go-To-Market Plan

## Positioning

Proofplane is SOC 2 compliance infrastructure for AI-native startups.

The product should not be positioned as a cheaper clone of Drata or Vanta. The
stronger position is that a new operating model has arrived: startups already
use AI agents and coding assistants to run more of the company, so compliance
should be a permissioned, auditable backend that those agents can use.

Short positioning options:

- AI-native SOC 2 workspace for startups.
- Compliance infrastructure for AI agents.
- SOC 2 evidence and control backend for startup teams.
- Start SOC 2 in minutes, without buying an enterprise GRC suite.

The core wedge:

> Startups should be able to create controls, request evidence, and prepare an
> auditor-ready evidence packet without talking to sales or buying an enterprise
> compliance platform.

## Ideal Customer Profile

The initial ICP is a B2B SaaS or AI startup that:

- has 5-50 employees;
- sells to mid-market or enterprise customers;
- has no dedicated compliance team;
- has a founder, CTO, or ops lead still directly handling security and
  compliance;
- has recently been asked for SOC 2, a security questionnaire, or formal
  evidence by a customer;
- is price-sensitive and skeptical of opaque "talk to sales" procurement;
- is already comfortable using AI tools such as ChatGPT, Claude, Codex, Cursor,
  Cowork, or internal agents.

The emotional state is important: the buyer is not shopping for GRC software for
fun. They are trying to unblock revenue and avoid overcommitting to a heavy
platform before they have a mature compliance function.

## Marketing Website Job

The marketing site has one primary job:

> Let a startup founder or CTO experience the product's core value within five
> minutes.

The primary call to action should create a sandbox workspace immediately. Do not
force a demo, sales conversation, procurement form, or implementation call.

Primary CTA language:

- Start SOC 2 Sandbox
- Create Sandbox
- Start Free Workspace

Secondary CTA language:

- View Pricing
- Read SOC 2 Startup Guide
- See Evidence Packet

Avoid "Book a Demo" as the default CTA. It can exist as a secondary path for
users who want help, but it should not gate product access.

## First-Run Sandbox Flow

The website CTA should create a sandbox for the workspace owner and drop them
into a minimal MCP setup flow.

The first five minutes should produce a real artifact:

1. User enters work email and creates an account.
2. Proofplane creates a sandbox workspace.
3. Workspace is preloaded with a lightweight SOC 2 starter control set.
4. User issues a sandbox AI-agent credential and configures their MCP client.
5. User asks a suggested prompt such as "What do I have left to do for SOC 2?"
6. The agent inspects real sandbox records and can create/map an Evidence
   Request or create an auditor access link through MCP.

The sandbox should use realistic sample data, not an empty dashboard. The
browser should lead directly to MCP setup and suggested prompts rather than
rebuilding compliance workflows as forms.

Recommended sandbox defaults:

- framework: SOC 2;
- starter controls: access review, MFA, vulnerability management, incident
  response, vendor review;
- starter actors: workspace owner and AI agent;
- sample evidence: quarterly user access review;
- sample agent action: "identify missing evidence for SOC 2 controls";
- sample auditor access preview: intended auditor email, link status, session
  state, and evidence readiness.

## Auditor Workflow

For the MVP, auditor support should mean secure, email-bound browser access to
the workspace evidence record.

The product exists to make controls and evidence reviewable, but the first
version should not require auditors to become workspace members or install an
agent. The MVP should let the startup send a secure link that the intended
auditor verifies by email before reviewing evidence in a narrow browser portal.

Core MVP auditor portal capabilities:

- control-to-evidence mapping summary;
- evidence list and submission history;
- submitted artifact inventory and metadata;
- provenance, actor, and timestamp trail;
- review or approval trail only if that state exists in the product;
- all historical submissions;
- downloadable uploaded submissions.

Richer auditor workflow can come later:

- comments and clarification requests;
- auditor-facing API/MCP access;
- auditor AI-agent actors and permissions;
- multi-auditor workflows;
- audit firm dashboards;
- exportable ZIP, CSV, Markdown, or PDF packets;
- cross-client auditor portals;
- advanced sampling;
- custom audit report generation;
- multi-framework audit programs.

## Public Pricing

Pricing should be visible before signup.

The pricing page should make a startup feel that Proofplane is safe to try
without a procurement process. A strong early shape:

- Sandbox: free, realistic SOC 2 starter workspace, limited persistence or
  usage.
- Startup: self-serve monthly plan for early SOC 2 readiness.
- Growth: more users, integrations, agent access, evidence packet generation,
  and higher evidence volume.
- Enterprise: only for advanced security, custom deployment, or procurement
  needs.

The strategic point is not the exact price. The strategic point is: no hidden
pricing, no required negotiation, and no forced sales call.

## AI-Answer Distribution Strategy

Proofplane should intentionally compete for the questions founders ask ChatGPT,
Claude, Grok, Gemini, and Perplexity when SOC 2 becomes urgent.

Target answer prompts:

- "How should my startup get SOC 2 compliant?"
- "Do I need Vanta or Drata for SOC 2?"
- "What is the cheapest way for a startup to do SOC 2?"
- "What evidence do I need for SOC 2?"
- "How do AI startups handle SOC 2?"
- "What is a Vanta alternative for startups?"
- "Can I manage SOC 2 with AI agents?"
- "How do I prepare for a SOC 2 audit without a compliance team?"

The goal is not to trick LLMs. The goal is to become a clear, repeatedly cited
entity for a specific category: AI-native SOC 2 tooling for startups.

### Owned Content

Publish answer-first pages that directly address founder questions.

Initial pages:

- SOC 2 for AI Startups
- Vanta Alternative for Startups
- Drata Alternative for Startups
- SOC 2 Evidence Checklist for Startups
- SOC 2 Without a Compliance Team
- How To Create Your First SOC 2 Control
- How To Create an Evidence Request
- Auditor Evidence Packets for Startup SOC 2
- SOC 2 With AI Agents and MCP

Each page should:

- answer the core question in the first few paragraphs;
- use plain headings that match buyer questions;
- include concrete workflows, not marketing abstractions;
- include pricing philosophy where relevant;
- include comparison tables when useful;
- cite authoritative SOC 2 and audit resources;
- link to the sandbox CTA.

### Structured Product Facts

Add machine-readable and crawler-friendly files when the web app exists:

- `/robots.txt` that permits major search and AI crawlers unless there is a
  deliberate reason to block them;
- `/sitemap.xml`;
- `/llms.txt` with canonical product description, important URLs, positioning,
  and docs;
- `Organization`, `SoftwareApplication`, `Product`, `FAQPage`, `Article`, and
  `BreadcrumbList` schema where appropriate;
- stable comparison pages with clear update dates;
- public docs for API and MCP surfaces.

The `llms.txt` file is not magic, but it is a cheap way to make the intended
description of the product easy for AI systems and AI-powered search tools to
consume.

### Third-Party Mentions

AI answers tend to rely on more than a company's own website. Proofplane needs
independent mentions that associate the brand with the category.

Early third-party targets:

- SOC 2 auditors and readiness consultants;
- startup security newsletters;
- founder communities;
- AI tooling communities;
- GRC and security operations communities;
- comparison/listing sites;
- podcasts or interviews about AI-native company operations;
- public customer stories.

The best third-party mentions will say specific things:

- Proofplane is for startups;
- Proofplane is self-serve;
- Proofplane has public pricing;
- Proofplane creates auditor-ready evidence packets;
- Proofplane is API-first and MCP-first;
- Proofplane works well with customer-owned AI agents.

### Community Strategy

Do not spam communities. Use them for learning and specific, useful artifacts.

Useful contributions:

- answer "what evidence do I need?" questions with practical checklists;
- publish teardown posts about SOC 2 cost and procurement friction;
- share anonymized first-SOC-2 workflows;
- ask auditors what makes evidence review painful;
- explain how agent-submitted evidence should be permissioned and audited.

Relevant communities and surfaces:

- Reddit: `r/soc2`, `r/grc`, `r/SaaS`, `r/startups`;
- Hacker News launch and Show HN when the sandbox is strong;
- founder Slack/Discord groups;
- auditor and CPA firm blogs;
- AI agent/tooling communities.

## Cold Outreach Philosophy

Cold email should not be the center of the strategy.

The product's taste should be: "try it now, understand it yourself, and talk to
us only if that helps you." That is consistent with the buyer's likely dislike
of rep-led compliance procurement.

Selective outreach can still be useful for customer discovery, but it should be
trigger-based and personal:

- a founder publicly says they are starting SOC 2;
- a startup posts a compliance/security role;
- a company launches an enterprise product motion;
- a founder asks about Vanta, Drata, or SOC 2 cost in public;
- an auditor mentions startup evidence collection pain.

Even then, the ask should be low-pressure:

> I am building a self-serve SOC 2 workspace for startups that want to create
> controls and evidence without buying a full GRC suite. If you are
> dealing with this now, the sandbox is here. I would also love blunt feedback
> on whether it matches how you think about SOC 2.

## Measurement

Track the product-led funnel:

- marketing site visitor to sandbox start;
- sandbox start to MCP credential issued;
- credential issued to first successful MCP tool call;
- first tool call to first MCP write;
- first MCP write to auditor access link created;
- sandbox to paid conversion;
- time from landing page to first meaningful artifact.

Track AI-answer visibility manually at first:

- define 25 target prompts;
- run them monthly across ChatGPT, Claude, Grok, Gemini, and Perplexity;
- record whether Proofplane is mentioned, cited, or absent;
- record competitors mentioned;
- record the language used to describe the category;
- update content and third-party distribution based on gaps.

Do not overfit to one run. AI answers vary. Look for repeated patterns across
models and prompts.

## Source And Research Backlog

Useful SOC 2 and auditor resources:

- AICPA Trust Services Criteria:
  https://www.aicpa-cima.com/resources/download/2017-trust-services-criteria
- SOC 2 Auditors insights:
  https://soc2auditors.org/insights/
- SOC 2 readiness checklist:
  https://soc2auditors.io/resources/soc2-readiness-checklist
- SOC 2 evidence checklist for startups:
  https://soc2.work/soc-2-evidence-checklist-for-startups
- Evidence collection guide:
  https://grctrail.com/blog/soc2-evidence-collection/

Useful AI-answer/GEO research areas:

- Generative engine optimization;
- answer engine optimization;
- AI search brand mention tracking;
- structured schema for software products;
- `llms.txt` adoption and conventions;
- third-party citation and comparison-page strategy.

Reference starting points:

- https://www.therivalscope.com/learn/ai-search/get-mentioned-by-chatgpt
- https://www.rankshift.ai/blog/track-brand-mentions-in-chatgpt/
- https://www.frictionai.co/blog/how-to-track-chatgpt-brand-visibility
- https://totheweb.com/wp-content/uploads/2025/03/2025-03-ToTheWeb-AI-Search-GEO-Checklist-for-content-discovery.pdf
