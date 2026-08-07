---
name: epics-and-tickets
description: Use when creating, organizing, or updating Proofplane project work as epics and tickets — breaking a spec or project into shippable tickets, drafting a ticket, writing an epic issue index, or working a ticket (marking progress, changing status, keeping the epic issue in sync). Covers the GitHub issue layout, the epic spec under docs/epics/, the lean ticket format, the ticket lifecycle, progress tracking, status vocabulary, and the definition of done.
---

# Epics and Tickets

Proofplane plans work as **epics** (a project) decomposed into **tickets** (the
individual, independently shippable units of work). The depth lives in one spec
per epic, versioned in the repo; tickets are lean, reviewable handoff units on
GitHub that link to it.

The guiding principle: **a ticket nobody reads is worthless.** Optimize for
handoff, not completeness. A reviewer should understand a ticket in 60–90 seconds.
If you are tempted to explain the _how_ in depth, that belongs in the spec — the
ticket links to it.

## Where things live

```
GitHub issues (proofplane/proofplane)
    "Epic: <Name>"                    # labels: epic, epic:<slug>
        └── ticket issues             # labels: ticket, epic:<slug>; attached as sub-issues

docs/epics/<epic-slug>/
    README.md                         # short pointer to the epic issue
    spec.md                           # the deep technical spec: rationale, schema, decisions
    ux.md                             # (optional) interface spec, only for epics with UI work
```

Specs are versioned with the code they describe, so they stay in the repo.
Tickets churn and are read by humans and agents outside a checkout, so they live
on GitHub. See `docs/agents/issue-tracker.md` for the tracker conventions and
`docs/agents/triage-labels.md` for the triage vocabulary.

The spec is the single source of truth for SQL, schema, type sketches, and
rationale. Do **not** duplicate that depth into tickets — link to the relevant
spec section instead, using a full `https://github.com/proofplane/proofplane/blob/main/...`
URL, since relative repo links do not resolve from an issue body.

### The epic spec

- **Fixed filename `spec.md`.** One per epic; the folder already names the topic,
  so no `<topic>-` prefix.
- **The word is "spec," never "design"** — "design" is ambiguous with UI/visual
  design. `spec.md` is the technical/engineering spec.
- **UI work goes in a sibling `ux.md`** (how the interface looks and behaves),
  added only when an epic has UI. Backend epics have just `spec.md`; full-stack
  epics have `spec.md` + `ux.md`. The two words never overlap.
- If an epic ever needs several reference docs, promote to a `spec/` folder — a
  later concern, not a rule to apply pre-emptively.
- **The spec is a living document — reconcile it when implementation deviates.**
  The spec is the source of truth, so it must describe what was *actually built*,
  not an approach the code abandoned mid-flight. When a ticket changes a design
  decision during the work (a different store, a dropped component, a simpler
  mechanism), update `spec.md` in the same change: correct the affected sections
  so nothing contradicts, and record the deviation **with its rationale** (the
  depth belongs in the spec). Mark it visibly — e.g. a short "Decision revision"
  note or an inline _(Revised during #NN — …)_ aside — rather than silently
  rewriting history, so the reasoning survives. Leave a one-line pointer in the
  ticket's Notes. Contradictory docs are worse than none: never let the spec keep
  advertising a path the implementation no longer follows.

## Naming and labels

- Epic folder: kebab-case slug (`auth-hierarchy-api`).
- Epic issue title: `Epic: <Short Title>`; labels `epic` and `epic:<slug>`.
- Ticket issue title: a short descriptive name, no number prefix — GitHub numbers
  it. Labels `ticket` and `epic:<slug>`, plus one triage label.
- Every ticket issue is attached to its epic issue as a **sub-issue**.

## The lean ticket format

Copy `templates/ticket-issue.md`. Keep it to roughly one screen. Use bullets, not
paragraphs. Sections, in order:

1. **Parent** — the epic issue reference.
2. **What to build** — 1–3 sentences: the end-to-end behavior this ticket makes
   work, from the user's perspective. Optionally a user story: _"As a `<role>`, I
   want `<x>` so that `<y>`."_
3. **Acceptance criteria** — a `- [ ]` checkbox list in **Given / When / Then**
   form, one scenario per line: _Given `<context>`, when `<action>`, then
   `<observable outcome>`_. This is the contract and the QA. Include at least one
   negative/rejection case and any "unchanged" guarantee for adjacent behavior you
   touch but don't change. Keep each scenario to a single checkbox so progress
   stays trackable; purely structural constraints (DI, dependencies) belong in the
   Definition of Done, not here.
4. **Tasks** — a `- [ ]` checkbox list of the implementation steps. This tracks
   progress across work sessions. Include a tests task.
5. **Blocked by** — the blocking ticket issues, or "None — can start
   immediately". Also set GitHub's native **blocked by** relationship; the section
   is for readers, the relationship is for tooling.
6. **Notes** (optional) — a few bullets for a key decision or gotcha, each
   pointing at the spec for detail. Never restate the spec. Use a Notes bullet for
   a deferral ("X is deferred to #NN") only when a reader might reasonably assume
   it is included.
7. **Spec** — a link to the spec section carrying the depth.

**Acceptance criteria vs Tasks:** acceptance criteria are the *contract* — what
must be true for the ticket to be done (required, stable, verified by tests).
Tasks are the *plan* — how the implementer gets there (fluid, a convenience for
tracking). One AC usually spans several tasks; rewriting tasks is not a scope
change, but rewriting AC is.

Do not add a scope / out-of-scope section — anything a ticket does not specify is
out of scope by default; capture a genuine deferral as a single Notes bullet
instead. Do not add separate "Tests" or "QA Guide" sections — testable acceptance
criteria are the QA, and a tests task covers the work. Do not inline SQL/schema
unless a single short snippet is genuinely the clearest way to state an acceptance
criterion. Avoid specific file paths — they go stale fast.

## Progress tracking

Two levels:

- **Within a ticket:** check off `- [ ]` items in Acceptance criteria and Tasks
  as work lands. This is how partial / multi-session progress is visible — most
  tickets are not one-shot.
- **Across the epic:** the epic issue's sub-issue list gives a live progress bar,
  and its body carries the ticket index and sequencing.

If a ticket needs more than ~6–8 tasks, or more than a few days of work, that is
the signal to **split it into smaller child tickets** rather than grow it. Small,
independently shippable tickets are the goal (INVEST: Independent, Negotiable,
Valuable, Estimable, Small, Testable).

## Status vocabulary

Implementation status is the issue's open/closed state:

- **Open** — not yet delivered. Unstarted and in-progress both look like this;
  the checkbox lists show which.
- **Closed as completed** — work complete, acceptance criteria met.

Triage is a separate axis, carried by the labels in
`docs/agents/triage-labels.md` (`needs-triage`, `needs-info`, `ready-for-agent`,
`ready-for-human`, `wontfix`). Changing a triage label never opens or closes an
issue. A ticket that cannot proceed stays open — say so in a comment naming the
blocker, and set the native blocked-by relationship.

## Working a ticket

A ticket is a living document — keep it current *as* you work, not only at the end.

1. **Pick up:** assign yourself and confirm it meets the Definition of Ready.
2. **As work lands:** check off `- [ ]` Tasks and Acceptance criteria the moment
   each becomes true — the checkboxes are the progress signal.
3. **If stuck:** comment with the blocker and the ticket or dependency it waits
   on, and set the native blocked-by relationship so it is visible.
4. **When the work is done:** close as completed once all acceptance criteria are
   checked.
5. **Keep in sync:** when a ticket's state changes, update the epic issue's ticket
   index if its note is now wrong — the epic issue is the at-a-glance view.
6. **Reconcile the spec:** if the implementation deviated from the spec, update
   `spec.md` to match what shipped (with the deviation's rationale) before closing
   the ticket — see "The spec is a living document" above.

Reflect reality, not aspiration: a ticket with a half-checked task list stays
open.

## Definition of Ready (before a ticket is picked up)

- "What to build" is clear (what the ticket delivers and why).
- Acceptance criteria are present and testable.
- Blocking tickets are listed and either closed or sequenced.
- The spec covers any non-obvious technical depth.

## Definition of Done (when a ticket is closed)

- All acceptance criteria are met and checked.
- New behavior is covered by tests.
- The spec reflects what was actually built — any mid-flight deviation from it has
  been reconciled into `spec.md` (with rationale), so no doc contradicts another.

## Epic issue and README

The epic **issue** carries the index. Use `templates/epic-issue.md`:

- One paragraph: what the project delivers and any core principle.
- A **Spec** section linking `spec.md` and its key sections.
- A **Tickets and sequencing** section: a `| Ticket | Status | Notes |` table
  referencing issues by number, then bullets on dependencies and what can
  parallelize.

The epic **README** in `docs/epics/<slug>/` is a short pointer, not a duplicate.
Use `templates/epic-readme.md`: the summary paragraph, a link to `spec.md`, and
a Tracking section linking the epic issue and the `epic:<slug>` label filter. It
carries no ticket table — that would drift from GitHub.

## Authoring workflow

1. **Epic vs ticket.** A project needing several shippable units → an epic issue
   plus a `docs/epics/<slug>/` folder with a README and a spec. A single unit
   within an epic → one ticket issue.
2. **New epic:** create the folder, write the spec (all depth lives here), the
   short README, then open the epic issue.
3. **Decompose** the spec's build order into small, independently shippable
   tickets, foundational ones first.
4. **Each ticket:** copy `templates/ticket-issue.md`, keep it ~one screen, link
   depth to the spec, write testable checkbox acceptance criteria and tasks.
5. **Publish in dependency order**, blockers first, so each ticket's "Blocked by"
   can reference real issue numbers. Attach each as a sub-issue, label it, and set
   its native blocked-by relationships.
6. **Maintain** the epic issue's index and the in-ticket checkboxes as work
   progresses.

## Quality checklist

- [ ] Epic issue exists, labeled `epic` + `epic:<slug>`; ticket labeled `ticket` + `epic:<slug>`.
- [ ] Ticket is attached to its epic as a sub-issue.
- [ ] Fits on ~one screen; bullets over paragraphs.
- [ ] "What to build" states what + why.
- [ ] Acceptance criteria are Given/When/Then checkboxes, testable, with a negative case.
- [ ] Tasks are checkboxes and include a tests task.
- [ ] Technical depth is in the spec and linked by full URL, not duplicated.
- [ ] "Blocked by" section matches the native blocked-by relationships.
- [ ] Exactly one triage label.
- [ ] Epic issue index updated with correct status and one-line note.
- [ ] Spec reconciled with what shipped — no section contradicts the built code.
