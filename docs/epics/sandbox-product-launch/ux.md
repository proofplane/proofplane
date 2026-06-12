# Sandbox Product Launch UX

## Core Principle

The first session must connect the customer's agent to real Proofplane records
and produce a useful compliance answer within five minutes. Avoid product
dashboards, CRUD forms, fake tours, and sales-gated calls to action.

## Entry Flow

1. Homepage primary CTA: **Start SOC 2 Sandbox**.
2. Auth0 login or account creation with minimum required identity fields.
3. Automatic create-or-resume sandbox provisioning.
4. Redirect to the MCP setup page.

Provisioning shows progress and can be safely retried. A partial failure offers
retry with the same workspace rather than creating another one.

## MCP Setup

The setup page has three jobs:

1. Issue or select the sandbox AI-agent credential.
2. Show copyable MCP configuration for supported clients.
3. Offer suggested prompts that exercise real Proofplane tools.

The raw key is shown exactly once. Configuration examples keep the secret in a
clearly marked placeholder or one-time copy control and do not place it in URLs,
analytics, or persistent browser storage.

## Suggested Prompts

Initial prompts should progress from read to write:

- "What do I have left to do for SOC 2 compliance?"
- "Which evidence requests are due or missing usable evidence?"
- "Create an access-review evidence request and map it to the relevant control."
- "Preview the auditor packet for my access-control evidence."

The page explains that the user's agent will call Proofplane tools and modify
real sandbox data. Proofplane does not embed a chat UI in the MVP.

## Returning Users

Returning owners see connection status guidance, actor/credential metadata,
rotate/revoke actions, setup instructions, and suggested prompts. They do not
see control, Evidence Request, mapping, or packet-editing forms.

## Public Pages

Use plain, answer-first copy. Pricing is reachable from every public page.
Comparison and guide pages lead to the same sandbox CTA and do not require
signup to read.

## Responsive And Accessible Behavior

The primary flow must work at narrow mobile and desktop widths, use keyboard
navigation, visible focus, semantic errors, and sufficient contrast. Loading,
empty, error, and retry states are part of each screen's acceptance criteria,
not deferred polish.
