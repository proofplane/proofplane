# Domain Docs

How the engineering skills should consume Proofplane’s domain documentation
when exploring the codebase.

## Before exploring, read these

- `CONTEXT.md` at the repository root.
- Relevant ADRs under `docs/adr/`.

If either location does not exist, proceed silently. Do not suggest creating it
upfront. The domain-modeling workflow creates domain documentation lazily when
terms or architectural decisions are resolved.

## File structure

Proofplane uses a single-context layout:

```
/
├── CONTEXT.md
├── docs/
│   └── adr/
└── src/
```

`CONTEXT.md` contains the shared domain vocabulary. System-wide architectural
decisions belong under `docs/adr/`.

## Use the glossary’s vocabulary

When output names a domain concept—in a ticket title, proposal, hypothesis, or
test name—use the term defined in `CONTEXT.md`. Do not substitute synonyms that
the glossary explicitly rejects.

If a needed concept is absent, reconsider whether the project already expresses
it differently. If it is a genuine gap, note it for the domain-modeling
workflow.

## Flag ADR conflicts

If proposed work contradicts an existing ADR, surface the conflict explicitly
instead of silently overriding the decision.
