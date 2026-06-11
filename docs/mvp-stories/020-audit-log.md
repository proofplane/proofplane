# 020 - Audit Log

> Superseded by structured application audit logging. Active planning lives in
> the [Reliability and Observability epic](../epics/reliability-observability/README.md)
> and the owning domain tickets. Proofplane will not add an `audit_events` query
> API for the MVP.

The replacement uses `tracing` records with `type = "audit_log"`, a restricted
Cloud Logging sink, and longer sink retention. Mutation success is logged after
commit, accepting the documented crash window between the database commit and
log emission. Audit analysis uses Cloud Logging rather than a Proofplane API.
