---
name: epics-and-tickets
description: Use when creating, organizing, updating, or implementing Proofplane project work as epics and tickets. Covers the GitHub issue layout, the epic issue that carries the technical depth, decomposition, ticket drafting, epic issue indexes, lifecycle status, progress tracking, epic reconciliation, and definition of done.
---

# Epics and Tickets

Proofplane plans work as **epics** decomposed into independently shippable
**tickets**. Technical depth lives in the epic issue. Tickets are lean,
reviewable handoff units that link to it.

Optimize for handoff, not completeness. A reviewer should understand a ticket
in 60-90 seconds. Detailed implementation rationale belongs in the epic issue.

## Where Things Live

Everything lives on GitHub. There are no epic files in the repository.

```text
GitHub issues (proofplane/proofplane)
    "Epic: <Name>"                    labels: epic, epic:<slug>
        body                          overview, technical depth, ticket index
        comments                      decision revisions, in date order
        ticket issues                 labels: ticket, epic:<slug>; attached as sub-issues
```

Do not create `docs/epics/`, a spec file, a `ux.md`, a README, or any other epic
file. That layout is retired. Humans and agents read this work outside a
checkout, so it lives where they read it. See `docs/agents/issue-tracker.md` for
tracker conventions and `docs/agents/triage-labels.md` for the triage vocabulary.

The epic issue body is the source of truth for schema, type sketches, decisions,
and rationale. Do not duplicate that depth in tickets. Link to the epic issue by
`#NN`, or to the permalink of the comment that carries the decision.

## Epic Issue Body

- Put the depth in the body, under headings a ticket can link to.
- Call it a "spec", never a "design".
- Put interface appearance and behavior in an optional `## UX` section of the
  same body.
- Correct the body when implementation changes a documented decision.
- Post a comment on the epic issue that records the revision and its rationale.
  The comment is timestamped and append-only, so an edit to the body never
  destroys the reasoning.
- Add a one-line pointer in the affected ticket's Notes when the epic changes.

## Naming And Labels

- Epic slug: kebab-case, such as `auth-hierarchy-api`. It names the
  `epic:<slug>` label, which is the only place the slug appears.
- Epic issue title: `Epic: <Short Title>`, labeled `epic` and `epic:<slug>`.
- Ticket issue title: a short descriptive name with no number prefix, because
  GitHub assigns the number. Labeled `ticket`, `epic:<slug>`, and one triage
  label.
- Attach every ticket issue to its epic issue as a sub-issue.

Preserve the punctuation already used by an existing epic when editing it.

## Ticket Format

Start from [templates/ticket-issue.md](templates/ticket-issue.md). Keep tickets
to roughly one screen and use bullets rather than long paragraphs.

Sections, in order:

1. Parent, referencing the epic issue.
2. What to build: the end-to-end behavior this ticket makes work, from the
   user's perspective.
3. Acceptance criteria as checkbox scenarios in Given/When/Then form.
4. Tasks as implementation checkboxes, including tests.
5. Blocked by, listing blocking issues or stating that none exist.
6. Optional Notes for decisions, gotchas, or explicit deferrals.
7. Spec, linking the epic issue section or comment that carries the depth.

Acceptance criteria are the stable contract. Tasks are the fluid implementation
plan. Include at least one rejection case and any unchanged guarantee for
adjacent behavior.

Also set GitHub's native blocked-by relationship for each blocking ticket. The
Blocked by section is for readers; the relationship is for tooling.

Do not add separate scope, out-of-scope, tests, or QA sections. Link to the epic
issue instead of inlining deep technical detail. Avoid specific file paths, which
go stale quickly.

Split a ticket when it grows beyond roughly 6-8 tasks or a few days of work.

## Status And Progress

Implementation status is the issue's open or closed state:

- Open: not yet delivered. An open ticket whose work has started also carries
  the `doing` label, and its checkbox lists show how far it has got.
- Closed as completed: all acceptance criteria are met. Remove `doing` when
  closing.

Triage is a separate axis carried by labels: `needs-triage`, `needs-info`,
`ready-for-agent`, `ready-for-human`, and `wontfix`. Changing a triage label
never opens or closes an issue.

A ticket that cannot proceed stays open. Comment with the blocker and set the
native blocked-by relationship.

Track progress at two levels. Within a ticket, check acceptance criteria and
task boxes as each becomes true. Across the epic, the sub-issue list gives a
live progress bar and the epic issue body carries the index and sequencing.

When working a ticket:

1. Assign yourself when implementation begins.
2. Check tasks and acceptance criteria as each becomes true.
3. Comment with a concise blocker when work cannot proceed.
4. Close as completed only after every acceptance criterion is checked.
5. Keep the epic issue ticket index synchronized.
6. Reconcile any implementation deviation into the epic issue before closing.
   Correct the body, then comment with the deviation and its rationale.

Reflect current reality. A partially checked ticket stays open.

## Ready And Done

A ticket is ready when:

- What to build states what and why.
- Acceptance criteria are testable.
- Blocking tickets are listed and sequenced.
- The epic issue contains non-obvious technical depth.

A ticket is done when:

- All acceptance criteria are met and checked.
- New behavior has tests.
- The epic issue describes what actually shipped.
- The epic issue index matches the ticket.

## Epic Issue

The epic issue is the whole epic. Start from
[templates/epic-issue.md](templates/epic-issue.md). Include:

- One paragraph describing the project and its core principle.
- A Spec section carrying the technical depth, under headings tickets can link
  to. Add a UX section only when the epic has UI work.
- A Tickets and sequencing section with a `Ticket | Status | Notes` table
  referencing issue numbers, then bullets on dependencies and parallel work.

Keep the ticket table in the body. The sub-issue list is the live progress bar,
and the table adds the sequencing notes that GitHub cannot express.

## Authoring Workflow

1. Decide whether the work is a multi-ticket epic or one ticket in an existing
   epic.
2. For a new epic, open the epic issue and write its body. All depth lives there.
3. Decompose the epic into small shippable tickets, foundational work first.
4. Draft each ticket from the template and link it to the relevant epic issue
   section.
5. Publish in dependency order, blockers first, so each Blocked by section can
   reference real issue numbers. Attach each ticket as a sub-issue, label it,
   and set its native blocked-by relationships.
6. Maintain ticket checkboxes and the epic issue as implementation progresses.

## Quality Checklist

- [ ] Epic issue labeled `epic` and `epic:<slug>`; ticket labeled `ticket` and `epic:<slug>`.
- [ ] Ticket attached to its epic as a sub-issue.
- [ ] Ticket is approximately one screen.
- [ ] What to build states what and why.
- [ ] Acceptance criteria are testable Given/When/Then checkboxes.
- [ ] Acceptance criteria include a negative case.
- [ ] Tasks include tests.
- [ ] Technical depth is in the epic issue and linked from the ticket, not duplicated.
- [ ] Blocked by section matches the native blocked-by relationships.
- [ ] Exactly one triage label.
- [ ] Epic issue index status and note are current.
- [ ] Epic issue matches the shipped implementation.
- [ ] No epic files were added to the repository.
