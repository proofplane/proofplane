---
name: Proofplane
description: SOC 2 compliance infrastructure for AI-native startups.
colors:
  ink: "#17201d"
  ink-muted: "#52615b"
  canvas: "#f7f3ea"
  surface: "#fcfaf4"
  surface-quiet: "#eee7da"
  line: "#d8cebd"
  primary: "#2f6f5e"
  primary-deep: "#1f5145"
  primary-soft: "#dbe9e3"
  signal: "#b45f3a"
  signal-soft: "#f0dfd4"
  code-bg: "#1f2623"
  code-text: "#edf3ec"
typography:
  display:
    fontFamily: "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
    fontSize: "clamp(2.5rem, 5vw, 4.75rem)"
    fontWeight: 650
    lineHeight: 0.98
    letterSpacing: "0"
  headline:
    fontFamily: "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
    fontSize: "clamp(1.75rem, 3vw, 2.75rem)"
    fontWeight: 620
    lineHeight: 1.05
    letterSpacing: "0"
  title:
    fontFamily: "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
    fontSize: "1.125rem"
    fontWeight: 640
    lineHeight: 1.25
    letterSpacing: "0"
  body:
    fontFamily: "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.55
    letterSpacing: "0"
  label:
    fontFamily: "Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 620
    lineHeight: 1.2
    letterSpacing: "0"
  mono:
    fontFamily: "'IBM Plex Mono', 'SFMono-Regular', Consolas, monospace"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: 1.45
    letterSpacing: "0"
rounded:
  sm: "4px"
  md: "6px"
  lg: "8px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "40px"
  xxl: "64px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.surface}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "10px 16px"
  button-primary-hover:
    backgroundColor: "{colors.primary-deep}"
    textColor: "{colors.surface}"
    rounded: "{rounded.md}"
    padding: "10px 16px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "10px 16px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "10px 12px"
  chip:
    backgroundColor: "{colors.primary-soft}"
    textColor: "{colors.primary-deep}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: "5px 8px"
---

# Design System: Proofplane

<!-- SEED -->

## 1. Overview

**Creative North Star: "The Audit Workbench"**

Proofplane should feel like a well-lit operations bench for compliance work: a
place where evidence, permissions, and agent activity are laid out with enough
structure to act on them immediately. The system should be calm and precise,
with dense information when useful, but never a wall of undifferentiated
compliance noise.

The visual language should support a product that is both trustworthy and
self-serve. It should make the workspace boundary, token permissions, MCP setup,
and auditor packet status visible without turning every concept into an
explanation panel.

The system rejects generic AI styling: no purple gradients, glass panels, neon
prompt boxes, glowing decorative orbs, or fake automation theater. It also
rejects enterprise GRC heaviness: no dead dashboards full of vanity charts before
the user has real evidence workflows.

**Key Characteristics:**

- Dense but readable product surfaces.
- Warm neutral canvas with exact, restrained operational color.
- Clear separation between human management actions and actor data-plane work.
- Token, permission, and packet states that are explicit and recoverable.
- Honest in-progress labeling for MCP and export capabilities that are not ready.

## 2. Colors

The palette is a warm workbench neutral system with green as the operational
action color and clay as the caution or branch-readiness signal.

### Primary

- **Workbench Green** (`#2f6f5e`): Primary actions, selected permission presets,
  successful setup progress, and active workspace indicators.
- **Deep Workbench Green** (`#1f5145`): Hover states, strong selected states, and
  high-contrast icon fills.
- **Soft Workbench Green** (`#dbe9e3`): Selected chips, low-emphasis success
  states, and scoped permission backgrounds.

### Secondary

- **Clay Signal** (`#b45f3a`): Pending, coming soon, branch-preview, or careful
  attention states. Use it to be honest, not alarmist.
- **Soft Clay Signal** (`#f0dfd4`): Low-emphasis warning surfaces and MCP preview
  annotations.

### Neutral

- **Proof Ink** (`#17201d`): Primary text and icon color.
- **Muted Proof Ink** (`#52615b`): Secondary text, metadata, timestamps, and
  helper copy.
- **Warm Canvas** (`#f7f3ea`): App and marketing page background.
- **Paper Surface** (`#fcfaf4`): Forms, repeated rows, packet previews, and
  contained tool surfaces.
- **Quiet Surface** (`#eee7da`): Subtle bands, inactive tabs, and table headers.
- **Ledger Line** (`#d8cebd`): Borders, dividers, input strokes, and table rules.
- **Code Slate** (`#1f2623`) and **Code Text** (`#edf3ec`): Token, environment
  variable, and MCP config snippets.

### Named Rules

**The Operational Accent Rule.** Workbench Green should mark action, selection,
and progress. Do not use it as decorative wash across the page.

**The Honest Signal Rule.** Clay Signal means pending, preview, caution, or
"not production ready yet." It should never become a fear color.

## 3. Typography

**Display Font:** Inter with system sans fallback.
**Body Font:** Inter with system sans fallback.
**Label/Mono Font:** IBM Plex Mono for token, API, and MCP config content, with
SFMono-Regular and Consolas fallback.

**Character:** Typography should feel crisp and operational. Product screens
should favor readable density; public explainer pages can use larger type, but
should still sound like a system being explained rather than a campaign.

### Hierarchy

- **Display** (650, `clamp(2.5rem, 5vw, 4.75rem)`, 0.98): Public explainer hero
  and rare first-run milestones only.
- **Headline** (620, `clamp(1.75rem, 3vw, 2.75rem)`, 1.05): Major page headings,
  onboarding step titles, and packet summary pages.
- **Title** (640, `1.125rem`, 1.25): Panel headings, table group labels, and
  settings sections.
- **Body** (400, `1rem`, 1.55): Primary reading text and product explanations.
  Keep line length around 65-75ch.
- **Label** (620, `0.8125rem`, 1.2): Buttons, tabs, chips, field labels, and
  compact metadata. Do not use negative letter spacing.
- **Mono** (500, `0.875rem`, 1.45): Tokens, workspace IDs, permission strings,
  endpoint paths, and MCP config snippets.

### Named Rules

**The Artifact-First Type Rule.** Large type introduces a workflow, not a
marketing abstraction. Data rows, packet gaps, and permission names should be
easier to scan than surrounding copy.

## 4. Elevation

Proofplane is flat by default and uses tonal layering, borders, and compact
state changes before shadows. Shadows are reserved for transient overlays,
focused menus, and raised copy panels where the user needs to preserve context.

### Shadow Vocabulary

- **Floating Low** (`0 10px 28px rgba(23, 32, 29, 0.10)`): Dropdown menus,
  popovers, and focused copy surfaces.
- **Floating High** (`0 22px 60px rgba(23, 32, 29, 0.16)`): Rare confirmation
  panels or narrow overlays. Avoid using this on page sections.

### Named Rules

**The Flat Workbench Rule.** Primary product surfaces stay flat at rest. Use
border, tone, and hierarchy before shadow.

## 5. Components

### Buttons

- **Shape:** Small-radius rectangles (`6px`) with stable height and clear icon
  affordances when an action has a familiar symbol.
- **Primary:** Workbench Green background, Paper Surface text, `10px 16px`
  padding, label typography.
- **Hover / Focus:** Deep Workbench Green on hover; `2px` focus ring using
  Clay Signal or Workbench Green depending on context.
- **Secondary / Ghost:** Paper Surface or transparent background, Proof Ink text,
  Ledger Line border, no decorative fill.

### Chips

- **Style:** Compact permission and status chips use soft tonal fills, not
  saturated badges.
- **State:** Selected permission chips use Soft Workbench Green and Deep
  Workbench Green text. Preview or pending chips use Soft Clay Signal and Clay
  Signal text.

### Cards / Containers

- **Corner Style:** `8px` maximum. Cards are for repeated objects, packet
  summaries, token success blocks, and compact setup panels.
- **Background:** Paper Surface on Warm Canvas, Quiet Surface for table headers
  and inactive areas.
- **Shadow Strategy:** Flat by default with Ledger Line borders. Use Floating Low
  only for popovers or focused copy controls.
- **Border:** `1px solid #d8cebd`.
- **Internal Padding:** `16px` for compact product panels, `24px` for onboarding
  and setup surfaces.

### Inputs / Fields

- **Style:** Paper Surface background, Ledger Line stroke, `6px` radius, body
  typography.
- **Focus:** Workbench Green border and a subtle `0 0 0 3px #dbe9e3` focus ring.
- **Error / Disabled:** Clay Signal for actionable issues; muted text and Quiet
  Surface for disabled controls.

### Navigation

- **Style:** Product navigation should be quiet and persistent, with workspace
  identity visible. Active states use Soft Workbench Green backgrounds and Deep
  Workbench Green text.
- **Mobile:** Collapse to a top bar with workspace switcher, setup progress, and
  a compact menu. Do not hide token safety or permission status behind deep
  navigation.

### Token Success Panel

The token success panel is a signature component. It must show the raw token
once, clearly label that it cannot be retrieved later, provide copy controls for
the token, environment variable, and MCP config snippet, and require the user to
acknowledge that the token has been saved before continuing.

### Auditor Packet Preview

Packet preview should look like an inspectable artifact, not a chart dashboard.
Rows should connect controls, evidence requests, latest submission status,
provenance, and gaps. Missing evidence is a clear work item, not a vague warning.

## 6. Do's and Don'ts

### Do:

- **Do** make the workspace, actor, token, and permission boundaries visible in
  onboarding and settings.
- **Do** give token creation job-based presets and show the granular permissions
  underneath.
- **Do** use Code Slate for config snippets and make copy actions obvious.
- **Do** label MCP branch-preview or coming-soon states with Clay Signal and
  plain language.
- **Do** prioritize evidence requests, control mappings, packet gaps, timestamps,
  and provenance over abstract dashboard metrics.
- **Do** keep cards to `8px` radius or less and use them for real objects, not
  decorative page sections.

### Don't:

- **Don't** make Proofplane look like a cheaper clone of Vanta, Drata, or a broad
  enterprise GRC suite.
- **Don't** force "Book a Demo" before the user can create a sandbox workspace.
- **Don't** use purple gradients, glass panels, glowing decorative orbs, neon
  prompt boxes, or vague AI automation claims.
- **Don't** build a dense compliance spreadsheet with no guided first-run path.
- **Don't** lead with vanity charts before the user has connected real evidence
  workflows.
- **Don't** use fear, panic, breach imagery, or alarmist red as the main sales
  language.
- **Don't** hide token permissions behind unclear "full access" labels.
- **Don't** use colored side-stripe borders, gradient text, decorative
  glassmorphism, or nested cards.
