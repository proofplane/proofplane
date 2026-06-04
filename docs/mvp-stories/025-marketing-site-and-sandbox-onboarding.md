# 025 - Marketing Site and Sandbox Onboarding

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

1. Show the starter control set.
2. Guide the user to create or edit one control.
3. Guide the user to create one evidence request.
4. Map the evidence request to the control.
5. Show an auditor-ready packet preview for the mapped control and evidence
   request.
6. Show the API/MCP shape an AI agent would use to inspect missing or stale
   evidence.

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
- The user can create or edit a control in the sandbox.
- The user can create an evidence request in the sandbox.
- The user can see an auditor-ready packet preview for the control and mapped
  evidence request.
- Public pricing is reachable before signup.
- The site includes `robots.txt`, `sitemap.xml`, and `llms.txt`.
- The key marketing pages have stable URLs and clear metadata for search and
  AI-answer tools.
- Funnel events are emitted for landing page view, sandbox start, first control
  creation, first evidence request creation, evidence packet preview, and paid
  conversion if billing exists.

## Tests

- Integration test verifies sandbox workspace provisioning.
- Integration test verifies sandbox seed data is workspace-scoped.
- Integration test verifies repeated CTA clicks resume or create the expected
  sandbox behavior without duplicate confusing workspaces.
- API test verifies sandbox users cannot access non-sandbox workspaces.
- Browser smoke test verifies the homepage CTA reaches the first-run flow.
- Browser smoke test verifies a user can create a control and evidence request.
- Snapshot or contract test verifies `llms.txt`, `robots.txt`, and `sitemap.xml`
  are served.

## QA Guide

1. Start the local application stack.
2. Open the marketing homepage.
3. Click `Start SOC 2 Sandbox`.
4. Complete minimal signup.
5. Verify a sandbox workspace is created.
6. Create or edit one control.
7. Create one evidence request.
8. Map the evidence request to the control.
9. Open the auditor-ready packet preview.
10. Verify funnel events are emitted.
11. Open `/llms.txt`, `/robots.txt`, and `/sitemap.xml`.
