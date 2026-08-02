# Proofplane Domain Context

Proofplane collects and evaluates evidence while preserving the workspace,
actor, and processing provenance needed to trust each artifact.

## Evidence Uploads

**Evidence submission**:
A single evidence file, its coverage window, and the provenance of the actor
that submitted it.
_Avoid_: Upload, attachment

**Human upload grant**:
A short-lived authority for a person to open the evidence upload experience and
manage submissions for one evidence target and coverage window.
_Avoid_: Machine upload grant, upload link

**Machine upload grant**:
A short-lived, single-purpose authority for an agent runtime to transfer one
declared evidence file into one preallocated evidence submission.
_Avoid_: Human upload grant, presigned upload

**Upload attempt**:
One transfer made under a machine upload grant; attempts may fail or race, but
at most one completes the grant.
_Avoid_: Evidence submission

## Document Processing

**Document**:
A file artifact whose lifecycle records staging, malware scanning,
finalization, failure, and archival for one evidence submission or policy.
_Avoid_: Attachment, blob

**Evidence document**:
A document owned by one evidence submission.
_Avoid_: Evidence submission

**Policy document**:
A document owned by one policy.
_Avoid_: Policy attachment

## Access and Identity

**Agent connection**:
A workspace-scoped relationship authorizing an agent client to act for a user
with an explicit set of permissions.
_Avoid_: Session, API key

**Auditor access grant**:
A workspace-scoped authority allowing an invited auditor to enter the auditor
portal for a bounded period.
_Avoid_: Auditor session, invitation token

**OAuth authorization flow**:
The lifecycle of one OAuth authorization request from preparation through
consent, cancellation, code issuance, and consumption.
_Avoid_: Login session, agent connection
