# Triage Labels

The skills speak in terms of five canonical triage roles. Proofplane records
these as a separate `Triage` field in local ticket files. The existing `Status`
field remains reserved for the `Todo` and `Done` implementation lifecycle.

| Label in mattpocock/skills | Value in our tracker | Meaning                                   |
| -------------------------- | -------------------- | ----------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue   |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent   |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation             |
| `wontfix`                  | `wontfix`            | Will not be actioned                      |

When a skill mentions a triage role, use the corresponding value from this
table without changing the ticket’s implementation status.
