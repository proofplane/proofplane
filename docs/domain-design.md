# Domain Design

Proofplane uses the workspace as the tenant boundary. Workspace-owned records are
the records a customer can create, edit, and authorize through workspace
membership. Canonical compliance reference data, such as frameworks and
framework requirements, is shared across workspaces.

Framework requirements are external compliance objectives. For SOC 2, these are
the Trust Services Criteria points a workspace may need to address. Proofplane
does not treat a requirement as something the workspace owns; the workspace owns
how it satisfies the requirement.

Controls are workspace-owned practices. A control can satisfy one framework
requirement, several requirements in the same framework, or requirements across
frameworks as the product grows. This is why controls have workspace-scoped
editable fields such as code, title, and description, while framework
requirements remain canonical reference data.

Evidence Requests are also workspace-owned. They describe recurring evidence
needs: what to collect, when it is due, how often it recurs, and how fresh the
evidence must be. An Evidence Request can support one or more controls, and a
control can be supported by many Evidence Requests.

Evidence scheduling and evidence freshness are related but distinct. `cadence`,
`schedule_anchor_at`, and `due_at` answer when evidence is expected: the
recurrence pattern, the anchor for that pattern, and the next concrete deadline.
`freshness_window_days` answers whether the currently submitted evidence is
still acceptable. It is intentionally separate because freshness may be shorter
or longer than the cadence. A quarterly request may require an export generated
within the last seven days, a monthly request may accept evidence for fourteen
days, and a one-time request may still need an expiration policy. When freshness
happens to match the cadence, the value may look redundant, but it represents a
different domain rule.

Evidence Submissions are concrete responses to an Evidence Request. A submission
records who submitted the evidence, when Proofplane received it, the evidence
coverage window, the source system, and the collection method. The coverage
window describes what period the evidence speaks for; `received_at` describes
when Proofplane accepted the submission. These are separate because evidence can
cover a past period even when it is uploaded later.

Evidence Attachments are the files or objects supplied with a submission. They
belong to a single Evidence Submission and carry object-storage location,
filename, content type, content length, SHA-256, and CRC32C checksums. Attachment
bytes are first stored in quarantine object storage, then the attachment record
and a pending scan record are created together. Clean scans atomically move the
attachment to `finalizing` and enqueue a finalization message; finalization
copies the object to its stable path before marking it `uploaded`.

Evidence Attachment Scans track the malware-scan state for each attachment. The
scan status starts as `pending` and can move to `clean`, `malicious`, or
`failed`, with scanner metadata and failure details recorded when available. A
scan request is emitted through the transactional outbox so the attachment
record, scan record, and worker message are committed atomically.

These relationships form a graph:

- `control <-> framework_requirement`
- `evidence_request <-> control`
- `evidence_request -> evidence_submission -> evidence_attachment -> evidence_attachment_scan`

They are first-class mappings rather than nested ownership. Nesting Evidence
Requests under frameworks or controls would imply a single parent and would make
common compliance workflows awkward: the same access review evidence can support
multiple controls, and the same control can map to multiple SOC 2 criteria.

API routes should therefore stay workspace-scoped rather than deeply nested under
frameworks or controls. A route such as
`/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/control-mappings`
expresses the tenant boundary and the relationship being edited without
pretending the Evidence Request belongs to one framework or one control.

Recurring submissions inherit mappings through Evidence Requests. Once an
Evidence Request is mapped to controls, each future submission for that request
can be evaluated against the same control graph without remapping every period's
evidence by hand.
