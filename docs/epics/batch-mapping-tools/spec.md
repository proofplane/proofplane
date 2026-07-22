# Batch Mapping Tools Spec

## Problem

Proofplane's two many-to-many relationships are only reachable through
single-pair MCP tools:

| Relationship         | Create                    | Remove                          |
| -------------------- | ------------------------- | ------------------------------- |
| evidence ↔ control   | `map_evidence_to_control` | `remove_evidence_control_mapping` |
| policy ↔ control     | `attach_policy_to_control`| `detach_policy_from_control`    |

Real compliance work is fan-out shaped. Onboarding a framework means mapping one
new piece of evidence across a dozen controls; authoring one control means
attaching the six policies that govern it. Today an agent issues a dozen
round trips, each its own transaction, each its own audit event. A failure
halfway through leaves a partial mapping set that nobody asked for, and the agent
has to reconcile what did and did not land.

## Core principle: batches go one way

A batch tool always fans out from **one anchor entity to many counterparts**. It
never accepts a list on both sides.

A tool with lists on both sides expresses a cross product. `map(evidence[3],
controls[4])` means twelve mappings, and the caller almost never means that —
they mean three unrelated fan-outs they happened to bundle. Worse, the rationale
that makes an evidence↔control mapping meaningful is per-pair, so a
both-sides-list tool would need a twelve-entry rationale matrix to stay
expressive. One-way batching keeps the anchor unambiguous, keeps the audit
record readable ("evidence X was mapped to these 12 controls"), and keeps the
argument shape flat.

Because each relationship is symmetric, one direction is not enough — the agent
that just created a control wants to attach evidence to it, and the agent that
just created evidence wants to map it to controls. So **each n↔n relationship
gets two tools, one per half**:

```
evidence ↔ control
  map_evidence_to_controls    anchor: evidence  →  many controls
  map_control_to_evidence     anchor: control   →  many evidence

policy ↔ control
  attach_policy_to_controls   anchor: policy    →  many controls
  attach_control_to_policies  anchor: control   →  many policies
```

and the same for removal:

```
  unmap_evidence_from_controls   unmap_control_from_evidence
  detach_policy_from_controls    detach_control_from_policies
```

Eight new tools. The four existing single-pair tools stay — they are the
one-item case, they are cheaper to describe to a model, and removing them would
break connected agents.

## Semantics

### All-or-nothing

Every batch is one database transaction and one authorization decision. If any
item in the batch is invalid, **nothing is written** and the call fails with the
offending IDs enumerated.

This was chosen over per-item partial success deliberately. A partial-success
result makes the agent responsible for parsing a mixed outcome and deciding what
to retry, and an LLM reconciling a half-applied batch is exactly the situation
where it invents a state that does not exist. All-or-nothing gives the agent one
bit to act on: it worked, or it did not and the world is unchanged. Retrying the
whole corrected batch is always safe.

The rejection payload names every bad ID, not just the first, so a single retry
can fix the whole batch. It follows Proofplane's existing MCP `problem` envelope
— the same shape `not_found`, `conflict`, and argument validation already use —
with the offending list under `ids` and the argument it belongs to under `field`:

```json
{
  "problem": {
    "code": "unknown_ids",
    "message": "control_ids contains unknown ids",
    "field": "control_ids",
    "ids": ["0f7c…", "91ab…"]
  }
}
```

_(Revised during ticket 001 — originally sketched as a bare
`{"error": "unknown_control_ids", "control_ids": [...]}`. Every other MCP error
in the codebase nests under `problem`, and an agent that has learned to read
`problem.code` should not need a second parser for batch failures.)_

### Duplicates within a batch

A repeated counterpart ID in one batch is a client bug, not an intent. Reject the
call with `duplicate_ids` rather than silently collapsing — collapsing would make
the response's mapping count disagree with the request's item count, which is
precisely the kind of quiet mismatch an agent mis-reports to its user. The
payload's `field` names which argument held the duplicates and `ids` lists each
repeated ID once, in first-seen order.

_(Revised during ticket 001 — originally specified as a per-counterpart code
such as `duplicate_control_ids`. One code plus a `field` key means all eight
tools share a single error contract, and the shared validator does not need a
code table keyed by counterpart type.)_

### Already-mapped pairs

Creating a mapping that already exists fails the batch, matching the existing
single-pair tools' conflict behavior. The `unmap`/`detach` batches are the mirror
image: removing a pair that is not mapped fails the batch. Neither tool is an
upsert. An agent that wants "make these the mappings" should read the current
mappings first — `list_evidence_control_mappings` and `get_control` already
expose them.

### Batch size

Cap at **50 items** per call, validated before any database work. The cap exists
to bound transaction duration and to keep a runaway agent from locking the
mapping tables; 50 comfortably exceeds the largest realistic fan-out (a control
mapped across every requirement of one framework). Over the cap returns
`batch_too_large` with the limit and the received count.

### Empty batch

An empty item list is rejected with `empty_batch`. Returning success for a no-op
teaches an agent that its malformed call worked.

## Implementation shape

The layering mirrors what is already in place; no new architectural seams.

**Repository** (`src/repository/controls.rs`, `src/repository/policies.rs`) —
add a batch method beside each existing single-pair method, sharing the anchor's
transaction. Each method, in order: check the anchor exists inside the workspace;
resolve every counterpart ID against the workspace with a read; if any is
missing, return the unknown-ID set without writing; otherwise insert each pair.
The existing single-pair methods are left untouched.

_(Revised during ticket 002 — originally sketched as a single
`INSERT ... SELECT FROM UNNEST(...)` with the unknown-ID set inferred from a short
`RETURNING` count and a follow-up re-query. That cannot work: an already-mapped
pair raises a unique violation on the insert, and **Postgres marks the whole
transaction failed after any statement error**, so the follow-up re-query — and
the eventual commit — would themselves error. Resolving IDs with plain reads
*before* any insert keeps the transaction healthy, names every unknown ID at once
(a failed insert would abort before the rest were checked), and lets an
already-mapped conflict roll the batch back cleanly. The batch is capped at 50, so
the extra per-item round trips are bounded and off any hot path.)_

Sketch for the evidence→controls half — one simple insert per resolved item:

```sql
INSERT INTO evidence_control_mappings (evidence_id, control_id, rationale)
VALUES ($1, $2, $3)
```

**Service** (`src/services/controls.rs`, `src/services/policies.rs`) — one
method per tool, wrapping the repository call in the same
`in_agent_connection_workspace_context` transaction helper the single-pair
methods use. Batch-shape validation (empty, size cap, duplicates) happens here,
before the transaction opens, via `domain::validate_batch` — one helper for all
eight tools, taking a key extractor so it serves both bare ID lists and
`{control_id, rationale}` objects. The service returns the set of affected
counterpart IDs, or a typed error carrying the offending IDs.

_(Revised during ticket 002 — the create tools' success response is lean:
`{ evidence_id, count, control_ids }`, not the full mapping objects. An agent that
needs each mapping's code, title, or rationale reads `list_evidence_control_mappings`
or the single-pair tool; echoing them back on every batch would bloat a 50-item
response for data the caller already holds.)_

**MCP** (`src/mcp/server/controls.rs`, `src/mcp/server/policies.rs`) — one
`#[tool]` per method, added to the existing `controls_tool_router` and
`policies_tool_router`. Authorization is unchanged: `WriteControls` for every
one of these, checked once per batch.

**Rationale on evidence mappings.** `evidence_control_mappings.rationale` is
per-pair and stays per-pair. Both evidence↔control batch tools therefore take a
list of `{control_id, rationale}` (or `{evidence_id, rationale}`) objects, not a
bare ID list. Policy↔control mappings carry no rationale, so those tools take
bare ID lists. The removal tools take bare ID lists in all four cases.

## Audit events

One event per **batch**, not per pair — a 40-item batch that emits 40 events
buries the audit log and misrepresents one human intent as forty. Use new event
names so existing single-pair events keep their exact meaning for anything
already querying them:

| Tool                          | Event                                   |
| ----------------------------- | --------------------------------------- |
| `map_evidence_to_controls`    | `evidence_control_mappings.created`      |
| `map_control_to_evidence`     | `evidence_control_mappings.created`      |
| `unmap_evidence_from_controls`| `evidence_control_mappings.deleted`      |
| `unmap_control_from_evidence` | `evidence_control_mappings.deleted`      |
| `attach_policy_to_controls`   | `policy_control_mappings.created`        |
| `attach_control_to_policies`  | `policy_control_mappings.created`        |
| `detach_policy_from_controls` | `policy_control_mappings.deleted`        |
| `detach_control_from_policies`| `policy_control_mappings.deleted`        |

Note the plural noun distinguishes them from the singular single-pair events.
Each event carries the anchor ID as its `object`, the full counterpart ID list
and the item count as metadata, and the tool name — so the direction is
recoverable from `tool` even though the two directions share an event name.

## Guide updates

`src/mcp/docs/controls-and-mappings.md` and `src/mcp/docs/policies.md` must
explain when to reach for a batch tool over its single-pair sibling, and must
state the all-or-nothing contract explicitly. An agent that does not know the
batch is atomic will write defensive retry logic that re-applies mappings.

## Build order

Evidence↔control first (higher traffic, and it carries the rationale
complication that the policy tools do not). Within each relationship, mapping
before unmapping. Repository, service, and MCP layer for one tool land together
as one ticket — a repository method with no caller is not independently
shippable.
