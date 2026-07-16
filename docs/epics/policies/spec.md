# Policies Spec

## Goal

Add workspace policies as a first-class part of the control domain. Compliance
officers manage policy metadata, control relationships, and one safely scanned
document through MCP-backed workflows. Auditors can browse every active policy
from the workspace-level portal and see the policies attached to each control.

The core principle is **one policy record, many controls, one current
attachment**. Policy metadata and mappings may exist before a document is
uploaded, while document bytes remain outside MCP and pass through the existing
quarantine, malware-scan, and finalization pipeline.

## Agreed Scope

A policy has:

- a workspace-scoped UUID;
- a required, case-insensitively unique name;
- an optional description;
- zero or more control mappings;
- at most one non-archived document attachment;
- created, updated, and optional archived timestamps.

“Compliance officer” is a product persona, not a new workspace membership
role. Existing workspace users operate through authorized MCP agent
connections. Policies do not add owner, version, effective date, approval
status, review cadence, or other governance fields in this epic.

## Persistence Model

Add three workspace-owned concepts:

```text
policies
  id UUID primary key
  workspace_id UUID -> workspaces.id
  name TEXT
  description TEXT nullable
  created_at TIMESTAMPTZ
  updated_at TIMESTAMPTZ
  archived_at TIMESTAMPTZ nullable

policy_control_mappings
  policy_id UUID -> policies.id
  control_id UUID -> controls.id
  created_at TIMESTAMPTZ
  primary key (policy_id, control_id)

policy_attachments
  id UUID primary key
  policy_id UUID -> policies.id
  filename/content_type/content_length
  object_key/checksum_sha256/checksum_crc32c
  archived BOOLEAN
  upload_status TEXT
  created_at TIMESTAMPTZ
```

Normalize policy names by trimming at the service boundary. Require a
non-empty name of at most 200 Unicode characters and an optional non-blank
description of at most 4,000 Unicode characters. A partial unique index on
`(workspace_id, lower(name)) WHERE archived_at IS NULL` provides race-safe
case-insensitive uniqueness among active policies. Archived names may be
reused because restore is not supported in v1.

A partial unique index on `policy_attachments(policy_id) WHERE archived =
false` enforces at most one current attachment even under concurrent uploads.
Attachment status uses the existing values exactly: `pending`, `finalizing`,
`uploaded`, `contains_virus`, and `failed`.

All policy and mapping mutations run in a workspace transaction. Mapping
inserts select both policy and control through the same workspace so a
cross-workspace relationship cannot be created. Archival retains mapping and
attachment rows for auditability, but normal reads exclude archived policies
and archived attachments.

## Policy Lifecycle And Mappings

Policy creation accepts name, optional description, and optional control IDs.
Creating a policy without controls or without a document is valid. If any
supplied control ID is malformed, duplicated, missing, or outside the caller's
workspace, the whole create operation fails without persisting the policy.

Policy updates change only name and description. Control mappings and the
attachment have dedicated operations and are never implicitly changed by a
policy update.

Attaching and detaching are explicit operations. A policy may map to many
controls and a control may map to many policies. Mappings carry no rationale or
other user-authored metadata. Duplicate attach is a conflict; detaching a
missing relationship returns not found. Both outcomes follow existing MCP
problem conventions and conceal cross-workspace existence.

Archiving is the only policy removal operation. It is rejected while the
current attachment is `pending` or `finalizing`; a policy with no attachment or
a terminal attachment may be archived. An archived policy is absent from
normal MCP reads and auditor views, cannot be changed or receive a new mapping
or upload grant, and cannot be restored in v1.

## MCP Contract

MCP remains the sole compliance data-plane. Add these tools:

```text
list_policies()
get_policy(policy_id)
create_policy(name, description?, control_ids?)
update_policy(policy_id, name, description?)
archive_policy(policy_id)
attach_policy_to_control(policy_id, control_id)
detach_policy_from_control(policy_id, control_id)
manage_policy_attachment(policy_id)
```

`list_policies` returns compact active-policy summaries ordered by
case-insensitive name and UUID: ID, name, optional description, attachment
presence/status, and mapped-control count. `get_policy` is the detailed read
and adds mapped control summaries plus safe attachment metadata. Neither tool
returns object keys, storage details, attachment bytes, archived history, or a
browser bearer URL.

`create_policy` returns the created detailed policy. `update_policy` returns
the updated detail. Mapping tools return compact policy and control identifiers.
`archive_policy` returns the archived policy ID and archival timestamp.

Policy reads require the existing `read_controls` permission. Creation,
updates, archival, mapping changes, and attachment-management grant
issuance require `write_controls`. No `read_policies` or `write_policies`
permission is added. Changing a policy mapping does not mutate the control and
does not require any permission beyond `write_controls`.

Validation, conflict, not-found, authorization, and dependency failures use
the established structured MCP problems. Missing and cross-workspace IDs are
indistinguishable. Successful meaningful reads and writes emit structured
audit events; failures do not emit success events.

## Attachment Lifecycle

Policy documents reuse the evidence attachment constraints, object storage,
quarantine, malware scanner, finalization behavior, file-size limits, and
accepted file types. The UI calls the file a “policy document,” but v1 adds no
policy-specific MIME allowlist.

The attachment pipeline must support both evidence and policy owners without
weakening existing evidence invariants. Extend worker messages and repository
work records with an explicit attachment owner kind and owner ID, using
`policy_attachment` as the aggregate type for policy work. Handlers select the
correct workspace-scoped record and final object-key namespace from that typed
owner; they must not infer ownership from an untrusted object key.

Policy attachment object keys use policy-specific quarantine and finalized
namespaces. Scanner and finalizer retries remain idempotent. Stale, duplicate,
wrong-owner, wrong-aggregate, and metadata-mismatched messages cannot advance a
record. Evidence attachment messages, keys, statuses, downloads, and audit
events remain unchanged.

There is no document version model. A second upload is rejected while a
non-archived attachment exists, regardless of its status. A user may delete an
`uploaded`, `contains_virus`, or `failed` attachment using the existing
archive-style behavior, after which a new upload is allowed. `pending` and
`finalizing` attachments cannot be deleted. Archived policy attachments are
hidden from normal reads and cannot be downloaded.

## Delegated Browser Management

`manage_policy_attachment(policy_id)` mirrors
`manage_evidence_submission_attachment`: it verifies an active policy in the
connection workspace and returns a short-lived, single-use bearer URL for a
human browser. The result includes the URL, expiry, policy ID, bearer-secret
classification, and human-browser intended use. The agent must present the URL
to the human and must not fetch, persist, summarize, or log it.

Persist a policy upload grant with grant ID, workspace ID, policy ID, issuing
user and agent connection provenance, issued time, five-minute expiry, and
optional redeemed time. Protect the URL with the existing upload-grant
keyring but a policy-specific audience/implicit assertion and typed claims.
Redemption atomically consumes the grant and establishes a separate HttpOnly,
SameSite cookie scoped to the policy upload routes and bounded by the original
expiry.

Add API-origin browser routes equivalent to:

```text
GET  /policy-attachment-uploads?token=<grant>
GET  /policy-attachment-uploads
POST /policy-attachment-uploads/files
POST /policy-attachment-uploads/files/{attachment_id}/archive
GET  /policy-attachment-uploads/files/{attachment_id}/download
```

The management UI should closely match the evidence attachment page in layout,
copy, responsive behavior, and accessibility. It shows policy identity,
attachment filename, size, and lifecycle status; offers one-file upload only
when no active attachment exists; offers archive only for terminal statuses;
and offers download only for `uploaded`. It includes no preview, drag-and-drop,
multi-file POST, polling, version history, or product login.

Every browser action reloads the session, policy, workspace, and attachment
eligibility. Expired, malformed, already redeemed, archived-policy,
wrong-scope, and missing state uses a generic unavailable response without
revealing workspace data. File bytes, URL secrets, cookies, and object keys are
never logged.

## Auditor Portal Read Model

Extend the workspace-scoped auditor read model with all active policies,
including unattached policies and policies with no document. Each policy
contains safe attachment metadata/status, download eligibility, and mapped
control summaries. Each control contains its attached active policy summaries.

The catalog and JSON read model order policies case-insensitively by name with
UUID tie-breaking. V1 returns the complete active set without search, filters,
or pagination. Archived policies and archived attachments are omitted.
Unavailable attachment states remain visible without download eligibility.
Object keys and storage details are never serialized.

Add a session-authenticated policy attachment download route under the auditor
portal. It streams only an `uploaded`, non-archived document after rechecking
the auditor session and backing grant, workspace, active policy, attachment
association/status, object metadata, and checksums. It uses the existing safe
download headers. Pending, finalizing, failed, malicious, archived, missing,
and cross-workspace requests return the portal's generic unavailable result.

## Auditor UX

Detailed browser behavior lives in [ux.md](./ux.md). At a high level, add
top-level portal navigation between “Framework requirements” and “Policies.”
The Policies page lists every active policy, and a policy detail page shows the
full description, mapped controls, document status, and eligible download.
Every control detail page gains an “Attached policies” section linking to
policy detail pages.

## Audit And Security

Emit identifier-only audit events for policy create, read, update, archive,
attach, detach, attachment grant issuance/redemption, attachment acceptance,
scan/finalization outcomes, attachment archive, management download, auditor
catalog/detail reads, and auditor document download. Follow existing event
naming and actor/client conventions.

Audit and application logs must not include policy descriptions, filenames
where the existing attachment contract excludes them, document contents,
checksums, object keys, bearer URLs/tokens, session cookies, or malware scanner
details. Policy and attachment IDs, control IDs, workspace ID, actor IDs,
operation, outcome, and coarse lifecycle status are allowed.

## Testing

Use unit tests for validation, typed attachment-owner messages, and status
transitions. Docker-backed integration tests cover:

- transactional policy creation and mapping rollback;
- active-name uniqueness and archived-name reuse;
- MCP schemas, permissions, tenant concealment, conflicts, and audit events;
- single-active-attachment enforcement and terminal-only archive;
- grant single-use, expiry, cookie scope, browser upload/download, and generic
  unavailable states;
- policy scan/finalization success, malicious files, failures, retries, and
  unchanged evidence attachment behavior;
- auditor catalog/control/detail composition, ordering, escaping, download
  eligibility, session revocation, and cross-workspace rejection.

## Deferred Work

Policy restore, hard deletion, document versions, multiple active attachments,
inline preview, search, filtering, pagination, bulk download, policy approval,
owners, effective dates, review cadence, acknowledgements, change history UI,
comments, and a compliance-officer policy SPA are deferred.

## Revisions

- 2026-07-15: Moved the policy attachment table and its single-active index into
  ticket 001 so policy archival can enforce its in-progress attachment guard.
  Ticket 003 still owns attachment lifecycle, storage, and worker behavior.
- 2026-07-15: Renamed the policy mutation from `replace_policy` to
  `update_policy`. The operation changes name and description only; it does not
  replace mappings, attachment state, or the policy record.
- 2026-07-15: Initial spec based on the policy product interview. Policies use
  existing control permissions, support many-to-many control mappings, expose
  one evidence-style attachment through a human browser flow, archive rather
  than hard-delete, and appear workspace-wide and per-control in the auditor
  portal.
