# Proofplane Glossary

- **Framework**: a global compliance standard, such as SOC 2, that groups requirements.
- **Requirement**: a framework statement with a stable ID. Use that ID when linking the requirement to a control.
- **Control**: the organization-specific safeguard that defines what must be proven.
- **Control mapping**: a link from a control to an evidence request. Its rationale explains why the requested proof supports the control.
- **Evidence request**: a recurring or one-time request for proof. Read its `collection_instructions`, due date, cadence, status, and freshness window before collecting anything.
- **Evidence submission**: the recorded proof for one evidence request. It carries a coverage window, source system, collection method, optional explanation, and the submitting agent connection's provenance.
- **Attachment**: a file associated with a submission. A human transfers file bytes through a short-lived browser flow; bytes never pass through MCP or the model.

Cadence determines when a request repeats. Coverage says which period a submission proves; freshness says how recent that proof must be. Keep both aligned with the request.

Auditor access is separate from attachment handling. An auditor access grant gives the named auditor a bearer-secret browser link to review evidence until expiry or revocation.
