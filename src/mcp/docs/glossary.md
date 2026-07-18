# Proofplane Glossary

- **Framework**: a global compliance standard, such as SOC 2, that groups requirements.
- **Requirement**: a framework statement with a stable ID. Use that ID when linking the requirement to a control.
- **Control**: the organization-specific safeguard that defines what must be proven.
- **Control mapping**: a link from a control to a piece of evidence. Its rationale explains why that proof supports the control.
- **Evidence**: something the organization must prove it does. Read its `collection_instructions` before collecting anything.
- **Evidence submission**: one file offered as proof for a piece of evidence. It carries the period it covers, the time it was received, and the submitting agent connection's provenance. A human transfers file bytes through a short-lived browser flow; bytes never pass through MCP or the model.

Coverage says which period a submission proves. Several submissions may share one coverage window when a single file cannot cover the period. To replace proof, archive a submission and upload another.

Evidence has no schedule of its own. If proof must be collected on a rhythm, say so in the evidence title or description, and set each submission's coverage window to the period it actually covers.

Auditor access is separate from uploading. An auditor access grant gives the named auditor a bearer-secret browser link to review evidence until expiry or revocation.
