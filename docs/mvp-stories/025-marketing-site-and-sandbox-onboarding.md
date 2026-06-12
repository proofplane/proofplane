# 025 - Marketing Site and Sandbox Onboarding

> Superseded as an active plan by the
> [Sandbox Product Launch epic](../epics/sandbox-product-launch/README.md).
> The current direction is MCP-first: the browser handles discovery, login,
> sandbox provisioning, and agent credential setup, while compliance work is
> performed through the customer's AI agent rather than browser forms.

## Goal

Create a product-led marketing surface that lets a startup founder or CTO start
using Proofplane without talking to sales.

The first experience should prove the core promise: a workspace owner can create
a SOC 2 control, create a mapped evidence request, and understand how the system
would package the result for an auditor in less than five minutes.

## Design

Build a small marketing site with a primary sandbox CTA.

Primary entry points:

- homepage
- pricing page
- SOC 2 for startups guide
- Vanta alternative for startups page
- Drata alternative for startups page
- SOC 2 evidence checklist page

Primary CTA:

- `Start SOC 2 Sandbox`

The CTA should create or resume a sandbox workspace for the authenticated
workspace owner. If the user is not authenticated, collect the minimum account
information needed to create the workspace and continue.

Sandbox provisioning should:

1. create a workspace;
2. create the workspace-owner actor;
3. seed a lightweight SOC 2 starter control set;
4. seed at least one evidence request mapped to a control;
5. create a sample AI-agent actor if the authorization model supports
   representing it safely;
6. create a sample auditor-ready packet preview;
7. mark the workspace as sandbox/demo mode;
8. route the user directly into the first-run product flow.

First-run flow:

1. Show the provisioned sandbox and AI-agent actor.
2. Issue and display an MCP credential once.
3. Show MCP configuration for supported agent clients.
4. Offer suggested prompts for inspecting gaps, creating an Evidence Request,
   mapping it to a control, and previewing an auditor packet.
5. Let the customer's agent perform those operations through MCP.

The sandbox should contain realistic data. Avoid empty dashboards and avoid
marketing-only walkthroughs that do not create product records.

## AI-Answer Readiness

The marketing site should include crawler-friendly product facts from launch.

Add:

- `robots.txt`
- `sitemap.xml`
- `llms.txt`
- product schema where appropriate
- organization schema
- FAQ schema on guide pages
- canonical URLs
- comparison pages with clear update dates

The owned content should answer founder-stage SOC 2 questions directly and link
back to the sandbox CTA.

## Acceptance Criteria

- A visitor can start a sandbox from the homepage without booking a demo.
- A new workspace owner can reach the product after providing only the minimum
  signup information.
- The sandbox workspace is created automatically.
- The sandbox includes starter SOC 2 controls and at least one mapped evidence
  request.
- The user can connect an MCP client using the sandbox actor.
- The agent can inspect the sandbox, create an Evidence Request, map it to a
  control, and request an auditor packet preview.
- The browser does not require compliance CRUD forms or an embedded chat UI.
- Public pricing is reachable before signup.
- The site includes `robots.txt`, `sitemap.xml`, and `llms.txt`.
- The key marketing pages have stable URLs and clear metadata for search and
  AI-answer tools.
- Funnel events are emitted for landing page view, sandbox start, credential
  issuance, MCP setup viewed, first successful tool call, first MCP write,
  evidence packet preview, and paid conversion if billing exists.

## Tests

- Integration test verifies sandbox workspace provisioning.
- Integration test verifies sandbox seed data is workspace-scoped.
- Integration test verifies repeated CTA clicks resume or create the expected
  sandbox behavior without duplicate confusing workspaces.
- API test verifies sandbox users cannot access non-sandbox workspaces.
- Browser smoke test verifies the homepage CTA reaches the first-run flow.
- MCP integration coverage verifies the suggested read/write prompt flows.
- Snapshot or contract test verifies `llms.txt`, `robots.txt`, and `sitemap.xml`
  are served.

## QA Guide

1. Start the local application stack.
2. Open the marketing homepage.
3. Click `Start SOC 2 Sandbox`.
4. Complete minimal signup.
5. Verify a sandbox workspace is created.
6. Issue the sandbox actor credential and configure an MCP client.
7. Ask what remains for SOC 2 compliance.
8. Ask the agent to create and map an Evidence Request.
9. Ask the agent to preview the auditor-ready packet.
10. Verify funnel events are emitted.
11. Open `/llms.txt`, `/robots.txt`, and `/sitemap.xml`.
