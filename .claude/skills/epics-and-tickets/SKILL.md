---
name: epics-and-tickets
description: Use when creating, organizing, or updating Proofplane project work as epics and tickets under docs/epics/ — breaking a spec or project into shippable tickets, drafting a ticket, writing an epic README index, or working a ticket (marking progress, changing status, keeping the README in sync). Covers the directory layout, the lean ticket format, the ticket lifecycle, progress tracking, status vocabulary, and the definition of done.
---

# Epics and Tickets

Proofplane plans work as **epics** (a project) decomposed into **tickets** (the
individual, independently shippable units of work). The depth lives in one spec
per epic; tickets are lean, reviewable handoff units that link to it.

The guiding principle: **a ticket nobody reads is worthless.** Optimize for
handoff, not completeness. A reviewer should understand a ticket in 60–90 seconds.
If you are tempted to explain the _how_ in depth, that belongs in the spec — the
ticket links to it.

## Directory layout

```
docs/epics/<epic-slug>/
    README.md                     # epic index: ordered ticket table + sequencing
    spec.md                       # the deep technical spec: rationale, schema, decisions
    ux.md                         # (optional) interface spec, only for epics with UI work
    tickets/
        001-<ticket-slug>.md
        002-<ticket-slug>.md
        ...
```

The spec is the single source of truth for SQL, schema, type sketches, and
rationale. Do **not** duplicate that depth into tickets — link to the relevant
spec section instead.

### The epic spec

- **Fixed filename `spec.md`.** One per epic; the folder already names the topic,
  so no `<topic>-` prefix. A fixed name means tickets and templates link to a
  deterministic path (`../spec.md`).
- **The word is "spec," never "design"** — "design" is ambiguous with UI/visual
  design. `spec.md` is the technical/engineering spec.
- **UI work goes in a sibling `ux.md`** (how the interface looks and behaves),
  added only when an epic has UI. Backend epics have just `spec.md`; full-stack
  epics have `spec.md` + `ux.md`. The two words never overlap.
- If an epic ever needs several reference docs, promote to a `spec/` folder — a
  later concern, not a rule to apply pre-emptively.

## Naming and numbering

- Epic folder: kebab-case slug (`auth-hierarchy-api`).
- Ticket file: `NNN-kebab-case-slug.md`, three-digit zero-padded, numbered in
  preferred integration order from `001`.
- Ticket `# H1` title: `NNN — Short Title` matching the filename.

## The lean ticket format

Copy `templates/ticket.md`. Keep it to roughly one screen. Use bullets, not
paragraphs. Sections, in order:

1. **Header line** — `**Status:** … · **Depends on:** … · **Spec:** <link>`.
   Status is one of the values below; link the spec section that carries the depth.
2. **Summary** — 1–3 sentences: what this delivers and why. Optionally a user
   story: _"As a `<role>`, I want `<x>` so that `<y>`."_
3. **Acceptance criteria** — a `- [ ]` checkbox list in **Given / When / Then**
   form, one scenario per line: _Given `<context>`, when `<action>`, then
   `<observable outcome>`_. This is the contract and the QA. Include at least one
   negative/rejection case and any "unchanged" guarantee for adjacent behavior you
   touch but don't change. Keep each scenario to a single checkbox so progress
   stays trackable; purely structural constraints (DI, dependencies) belong in the
   Definition of Done, not here.
4. **Tasks** — a `- [ ]` checkbox list of the implementation steps. This tracks
   progress across work sessions. Include a tests task.
5. **Notes** (optional) — a few bullets for a key decision or gotcha, each
   pointing at the spec for detail. Never restate the spec. Use a Notes bullet for
   a deferral ("X is deferred to NNN") only when a reader might reasonably assume
   it is included.

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
criterion.

## Progress tracking

Two levels, both git-tracked:

- **Within a ticket:** check off `- [ ]` items in Acceptance criteria and Tasks
  as work lands. This is how partial / multi-session progress is visible — most
  tickets are not one-shot.
- **Across the epic:** the README ticket table carries a **Status** column for
  ticket-level state.

If a ticket needs more than ~6–8 tasks, or more than a few days of work, that is
the signal to **split it into smaller child tickets** rather than grow it. Small,
independently shippable tickets are the goal (INVEST: Independent, Negotiable,
Valuable, Estimable, Small, Testable).

## Status vocabulary

- **Todo** — ready to pick up, not started.
- **Doing** — actively being implemented (some tasks checked).
- **Blocked** — cannot proceed; note the blocker.
- **Done** — work complete, acceptance criteria met.

## Working a ticket

A ticket is a living document — keep it current *as* you work, not only at the end.

1. **Pick up:** move `Todo` → `Doing` when you start (it should already meet the
   Definition of Ready).
2. **As work lands:** check off `- [ ]` Tasks and Acceptance criteria the moment
   each becomes true — the checkboxes are the progress signal.
3. **If stuck:** set `Blocked` and add a one-line blocker (and the ticket or
   dependency it waits on) so it is visible.
4. **When the work is done:** set `Done` once all acceptance criteria are checked.
5. **Keep in sync:** whenever a ticket's status changes, update the epic README's
   Status column to match — the README is the at-a-glance view.

Reflect reality, not aspiration: a ticket with a half-checked task list is
`Doing`, not `Done`.

## Definition of Ready (before a ticket is picked up)

- Summary is clear (what the ticket delivers and why).
- Acceptance criteria are present and testable.
- Dependencies are listed and either Done or sequenced.
- The spec covers any non-obvious technical depth.

## Definition of Done (when a ticket is Done)

- All acceptance criteria are met and checked.
- New behavior is covered by tests.

## Epic README

Use `templates/epic-readme.md`:

- One paragraph: what the project delivers and any core principle.
- Link to the spec for full rationale.
- A **Tickets** table: `| Ticket | Status | Notes |`, ticket column linking to the
  file, notes one line each.
- A **Sequencing** section: dependencies and what can parallelize.

## Authoring workflow

1. **Epic vs ticket.** A project needing several shippable units → an epic folder
   with a README and a spec. A single unit within an epic → one ticket.
2. **New epic:** create the folder, write the spec (all depth lives here), then
   the README with the full ticket table (rows may be `Todo` placeholders).
3. **Decompose** the spec's build order into small, independently shippable
   tickets, foundational ones first.
4. **Each ticket:** copy `templates/ticket.md`, keep it ~one screen, link depth to
   the spec, write testable checkbox acceptance criteria and tasks.
5. **Maintain** the README Status column and the in-ticket checkboxes as work
   progresses.

## Quality checklist

- [ ] Filename and `# H1` match (`NNN — Title`).
- [ ] Fits on ~one screen; bullets over paragraphs.
- [ ] Summary states what + why.
- [ ] Acceptance criteria are Given/When/Then checkboxes, testable, with a negative case.
- [ ] Tasks are checkboxes and include a tests task.
- [ ] Technical depth is in the spec and linked, not duplicated.
- [ ] Header has Status, Depends on, and Spec link.
- [ ] README table updated with correct status and one-line note.
