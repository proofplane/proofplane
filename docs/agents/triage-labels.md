# Triage Labels

The skills speak in terms of five canonical triage roles. Proofplane records
these as GitHub labels, which map one-to-one onto the vocabulary in
mattpocock/skills.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

Triage is independent of implementation status. Implementation status is the
issue's open/closed state: open means not yet delivered, closed as completed
means done. Applying or changing a triage label never opens or closes an issue.

A triage label is at most one per issue. Epic issues are containers and normally
carry no triage label; triage applies to the ticket issues beneath them.

```bash
gh issue edit 98 --add-label ready-for-agent --remove-label needs-triage
```

## The `doing` label

`doing` is not a triage role. It is an implementation-progress marker meaning
work on an open ticket has started, and its acceptance-criteria and task
checkboxes show how far it has got. Remove it when the issue closes.

It exists because open/closed alone cannot distinguish an untouched ticket from
one that is half-built, and that difference is only otherwise visible by opening
each issue.
