---
name: epics-and-tickets
description: Use when creating, organizing, updating, or implementing Proofplane project work as epics and tickets under docs/epics/. Covers decomposition, ticket drafting, epic indexes, lifecycle status, progress tracking, spec reconciliation, and definition of done.
---

# Epics and Tickets

Proofplane plans work as **epics** decomposed into independently shippable
**tickets**. Technical depth lives in one spec per epic; tickets are lean,
reviewable handoff units that link to it.

Optimize for handoff, not completeness. A reviewer should understand a ticket
in 60-90 seconds. Detailed implementation rationale belongs in the spec.

## Directory Layout

```text
docs/epics/<epic-slug>/
    README.md
    spec.md
    ux.md
    tickets/
        001-<ticket-slug>.md
        002-<ticket-slug>.md
```

`ux.md` is optional and only applies to epics with UI work.

The spec is the source of truth for schema, type sketches, decisions, and
rationale. Do not duplicate that depth in tickets.

## Epic Spec

- Use the fixed filename `spec.md`.
- Call it a "spec", never a "design".
- Put interface appearance and behavior in optional `ux.md`.
- Reconcile the spec when implementation changes a documented decision.
- Record revisions and their rationale visibly instead of silently rewriting
  history.
- Add a one-line pointer in the affected ticket's Notes when the spec changes.

## Naming

- Epic folder: kebab-case, such as `auth-hierarchy-api`.
- Ticket file: `NNN-kebab-case-slug.md`, zero-padded and ordered by preferred
  integration sequence.
- Ticket H1: `NNN - Short Title`, matching the filename and number.

Preserve the punctuation already used by an existing epic when editing it.

## Ticket Format

Start from [templates/ticket.md](templates/ticket.md). Keep tickets to roughly
one screen and use bullets rather than long paragraphs.

Sections, in order:

1. Header with `Status`, `Depends on`, and a link to the relevant spec section.
2. Summary of what the ticket delivers and why.
3. Acceptance criteria as checkbox scenarios in Given/When/Then form.
4. Tasks as implementation checkboxes, including tests.
5. Optional Notes for decisions, gotchas, or explicit deferrals.

Acceptance criteria are the stable contract. Tasks are the fluid implementation
plan. Include at least one rejection case and any unchanged guarantee for
adjacent behavior.

Do not add separate scope, out-of-scope, tests, or QA sections. Link to the spec
instead of inlining deep technical detail.

Split a ticket when it grows beyond roughly 6-8 tasks or a few days of work.

## Status And Progress

Use only:

- `Todo`: ready, not started.
- `Doing`: actively being implemented.
- `Blocked`: cannot proceed; state the blocker.
- `Done`: all acceptance criteria are met.

When working a ticket:

1. Change `Todo` to `Doing` when implementation begins.
2. Check tasks and acceptance criteria as each becomes true.
3. Set `Blocked` and add a concise blocker when work cannot proceed.
4. Set `Done` only after every acceptance criterion is checked.
5. Keep the epic README status table synchronized.
6. Reconcile any implementation deviation into `spec.md` before marking Done.

Reflect current reality. A partially checked ticket is `Doing`, not `Done`.

## Ready And Done

A ticket is ready when:

- The summary states what and why.
- Acceptance criteria are testable.
- Dependencies are listed and sequenced.
- The spec contains non-obvious technical depth.

A ticket is done when:

- All acceptance criteria are met and checked.
- New behavior has tests.
- The spec describes what actually shipped.
- The epic README status matches the ticket.

## Epic README

Start from [templates/epic-readme.md](templates/epic-readme.md). Include:

- One paragraph describing the project and its core principle.
- A link to `spec.md`.
- A `Ticket | Status | Notes` table.
- A Sequencing section explaining dependencies and parallel work.

## Authoring Workflow

1. Decide whether the work is a multi-ticket epic or one ticket in an existing
   epic.
2. For a new epic, write the spec first, then the README ticket index.
3. Decompose the spec into small shippable tickets, foundational work first.
4. Draft each ticket from the template and link it to the relevant spec section.
5. Maintain ticket checkboxes, ticket status, README status, and the spec as
   implementation progresses.

## Quality Checklist

- [ ] Filename and H1 number/title agree.
- [ ] Ticket is approximately one screen.
- [ ] Summary states what and why.
- [ ] Acceptance criteria are testable Given/When/Then checkboxes.
- [ ] Acceptance criteria include a negative case.
- [ ] Tasks include tests.
- [ ] Technical depth is linked from the spec rather than duplicated.
- [ ] Header includes Status, Depends on, and Spec.
- [ ] README status and note are current.
- [ ] Spec matches the shipped implementation.
