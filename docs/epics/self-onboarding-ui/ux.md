# Self-Onboarding UI UX

## Goal

The UI should let a founder understand Proofplane and reach a useful workspace
state in minutes: workspace created, scoped token issued, MCP setup preview
visible, and a clear picture of evidence and packet readiness.

## Visual Direction

Use the "Audit Workbench" system from [DESIGN.md](../../../DESIGN.md):

- warm neutral canvas;
- restrained workbench green for action and progress;
- clay signal for pending, preview, or unavailable states;
- flat product surfaces with borders before shadows;
- compact, readable panels instead of decorative card grids.

Avoid the anti-references from [PRODUCT.md](../../../PRODUCT.md): generic AI
gradients, glass panels, glowing prompt boxes, enterprise GRC heaviness, vanity
charts, and unclear full-access tokens.

## First-Run Flow

1. Public explainer page.
2. Auth0 signup/login.
3. Workspace creation.
4. Token permission preset selection.
5. One-time token success state.
6. MCP setup preview.
7. Workspace home.

Do not show setup-mode choices until the backend supports them.

## Public Explainer

The public page should answer three questions quickly:

- What is Proofplane?
- Why is it different from a GRC dashboard?
- What happens when I start workspace setup?

Primary CTA: `Start workspace setup`.

Secondary actions can point to pricing philosophy or docs, but should not
replace the self-serve CTA.

## Workspace Creation

The workspace step should ask only for backend-supported fields:

- workspace name.

After creation, show the workspace ID in a compact metadata area. Users who
belong to multiple workspaces should see the active workspace boundary clearly.

## Token Creation

The permission picker should use presets first and granular permissions second.
Each preset should explain the job it enables, then list the exact permission
strings underneath.

The one-time token success state must include:

- raw token display;
- copy token action;
- copy environment variable action;
- copy MCP config preview action;
- acknowledgement that the token was saved;
- revoke/regenerate path.

## MCP Setup Preview

Use honest labels:

- `Ready` for UI or API steps that work now.
- `Preview` for copy and guidance that depends on branch work.
- `Waiting on MCP Server` for actions blocked by the MCP epic.

Suggested prompts should be concrete:

- `What evidence is missing for SOC 2?`
- `Show me the latest evidence for MFA.`
- `Create an evidence submission for the quarterly access review.`
- `Preview an auditor packet and list the gaps.`

## Workspace Home

The home screen should be an operational snapshot, not analytics:

- setup progress;
- token and MCP readiness;
- starter controls;
- evidence request status;
- packet preview or unavailable state;
- suggested agent prompts.

Rows and statuses should connect the object, the current state, and the next
action. Missing evidence is a work item, not a warning decoration.

## Accessibility

The first-run path must be keyboard usable. Focus rings should be visible.
Status must not rely on color alone. Copyable code blocks must have labels that
screen readers can understand.

Reduced motion should keep the experience fully functional, with only simple
opacity or transform transitions disabled when requested.
