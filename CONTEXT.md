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
