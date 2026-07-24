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

_(Extended during PR #67 review — the rule now also covers `create_control` and
`replace_control`, whose `framework_requirement_ids` previously collapsed
repeats silently through `ON CONFLICT DO NOTHING`. They are not batch tools:
an empty list is legal there, and the size cap does not apply, so they reuse
only the duplicate scan (`domain::duplicate_ids`) and report it the way they
already report unknown requirement ids — `validation_failed` with a
`framework_requirement_ids` field issue naming each repeated ID — rather than
the batch tools' `duplicate_ids` payload.)_

### Already-mapped pairs

Creating a mapping that already exists fails the batch, matching the existing
single-pair tools' conflict behavior. The `unmap`/`detach` batches are the mirror
image: removing a pair that is not mapped fails the batch with `not_mapped_ids`,
listing every counterpart id that was not mapped. Neither tool is an upsert. An
agent that wants "make these the mappings" should read the current mappings
first — `list_evidence_control_mappings` and `get_control` already expose them.

_(Revised during ticket 004 — `not_mapped_ids` is a new code, kept distinct from
`unknown_ids`. A removal batch can fail two ways that read alike but demand
opposite corrections: the counterpart id does not exist in the workspace (the
agent has the wrong id) versus it exists but carries no such mapping (the id is
right and there is nothing to remove). Collapsing both into `unknown_ids` would
send an agent hunting for a control it can plainly see in `list_controls`. The
removal statement classifies both cases in the same pass that deletes — see the
Implementation shape revision below.)_

_(Revised during ticket 008 — **every batch tool, create and removal alike, now
reports all its failing-id categories together in one `batch_rejected` payload**, so
a caller learns exactly what failed and why in a single response and one corrected
retry can fix a batch that failed several ways at once. The payload always carries a
list per category the tool can produce, each key present (possibly `[]`) with at least
one non-empty:_

- _create/attach tools: `unknown_ids` + `already_mapped_ids` (plus `archived_ids` for
  `attach_control_to_policies`, whose counterparts are policies)._
- _removal/detach tools: `unknown_ids` + `not_mapped_ids` (plus `archived_ids` for
  `detach_control_from_policies`)._

_This replaced the earlier mix of a standalone `unknown_ids` code plus an opaque
`*_mapping_exists` conflict on the create side, and standalone
`not_mapped_ids`/`archived_ids` codes on the removal side — those top-level codes are
gone; the same names now appear only as keys inside `batch_rejected`. The per-category
classification is unchanged; only how the buckets are surfaced changed. An
already-mapped pair is now enumerated (see the Implementation shape revision below for
how the create path detects it without a conflicting insert). Batch-shape codes
(`empty_batch`, `batch_too_large`, `duplicate_ids`) are argument validation and stay
their own codes.)_

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
resolve every counterpart ID against the workspace with one set-based read; if
any is missing, return the unknown-ID set without writing; otherwise insert every
pair in one statement. The existing single-pair methods are left untouched.

_(Revised during ticket 002 — originally sketched as a single
`INSERT ... SELECT FROM UNNEST(...)` with the unknown-ID set inferred from a short
`RETURNING` count and a follow-up re-query. That cannot work: an already-mapped
pair raises a unique violation on the insert, and **Postgres marks the whole
transaction failed after any statement error**, so the follow-up re-query — and
the eventual commit — would themselves error. Resolving IDs with plain reads
*before* any insert keeps the transaction healthy, names every unknown ID at once
(a failed insert would abort before the rest were checked), and lets an
already-mapped conflict roll the batch back cleanly.)_

_(Revised again after ticket 004 — **the resolve-before-insert order above is
unchanged and still required; only the per-item looping is gone.** Both steps are
now set-based: one `WHERE id = ANY($1)` read resolves every counterpart, and one
`INSERT ... SELECT FROM unnest($2::uuid[], $3::text[])` writes every pair, so a
create batch is 3 statements instead of 1 + 2N — 101 round trips for a 50-item
batch became 3. Nothing about the failure contract moves: the multi-row insert
raises the same `evidence_control_mappings_pkey` unique violation, and the
unknown-ID set is still named in full, in request order, before any write.
`insert_policy_control_mappings` was already set-based; note that it may infer
its unknown IDs from a short `RETURNING` count only because it inserts against a
brand-new policy where no duplicate can exist — the evidence↔control halves
cannot, for the reason above.)_

_(Revised during ticket 008 — **the create/attach batch methods now read existing
mappings before inserting, too**, so an already-mapped pair is enumerated in
`already_mapped_ids` rather than surfacing as an opaque `*_mapping_exists` conflict
that names no ids. Each create batch runs three reads (anchor, counterparts in
workspace, existing mappings among the requested counterparts), classifies every
requested id into `unknown` / `already_mapped` (and `archived` for policy
counterparts), and returns the combined rejection **before** any insert — so the
insert only ever runs against ids known to be insertable, and the unique-violation
path is now just a race fallback. Rejections travel as `Error::BatchRejected` on
both create and removal paths, because `in_agent_connection_workspace_context`
commits on `Ok`; the repository methods therefore return `Ok(None)` for a missing
anchor, `Ok(Some(ids))` on success, and `Err(BatchRejected(..))` for an enumerated
rejection.)_

_(Revised during ticket 004 — **the removal methods do not resolve ids first, and
must not report a rejection as an `Ok` value.** The read-first order above exists
only because a conflicting `INSERT` aborts the transaction; a `DELETE` conflicts
with nothing, so one statement can remove the mappings and classify every
requested id in the same pass — 2 round trips for a 50-item batch instead of 52.
The catch is that the rejection is then discovered *after* the write, and
`in_agent_connection_workspace_context` **commits whenever the operation returns
`Ok`** — so a rejection returned as an outcome value would commit the very
deletes it was rejecting. Rejections therefore travel as
`Error::BatchRejected(BatchRejection)`, and the rollback the transaction already
provides is what makes the batch atomic. The service turns that error back into
its typed outcome via `From`.)_

Sketch for the evidence→controls removal half — one statement that deletes and
classifies, so an id the workspace lacks stays distinguishable from one it has
but never mapped:

```sql
WITH requested AS (SELECT unnest($2::uuid[]) AS control_id),
removed AS (
    DELETE FROM evidence_control_mappings m ...
      AND c.id IN (SELECT control_id FROM requested)
    RETURNING m.control_id
)
SELECT r.control_id,
       EXISTS (SELECT 1 FROM controls c
               WHERE c.id = r.control_id AND c.workspace_id = $3) AS control_exists,
       EXISTS (SELECT 1 FROM removed WHERE removed.control_id = r.control_id) AS was_removed
FROM requested r
```

Sketch for the evidence→controls create half — one read that resolves every
control, then one insert that writes every pair:

```sql
SELECT id FROM controls WHERE id = ANY($1) AND workspace_id = $2;

INSERT INTO evidence_control_mappings (evidence_id, control_id, rationale)
SELECT $1, requested.control_id, requested.rationale
FROM unnest($2::uuid[], $3::text[]) AS requested(control_id, rationale)
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
