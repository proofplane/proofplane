# Policies

Policies record the organization’s rules. Controls define the safeguards that must be proven, and policy-control mappings show which policies govern each control. A policy may map to many controls, a control may map to many policies, and either may exist before a mapping is created.

Start with `list_policies` and `get_policy`. Use `create_policy` or `update_policy` for policy metadata, then use `attach_policy_to_control` and `detach_policy_from_control` to manage relationships without changing the controls themselves. Archive a policy only when it should disappear from active compliance and auditor views.

When you have several relationships to make or break at once, reach for a batch tool instead of one call per pair. A batch fans out from a single anchor to many counterparts, so each direction is its own tool: `attach_policy_to_controls` attaches one policy to many controls, and `attach_control_to_policies` attaches one control to many policies; `detach_policy_from_controls` and `detach_control_from_policies` are the removal halves. There is no both-sides form, and unlike evidence mappings these carry no per-pair rationale. Only active policies can be attached or detached — an archived policy anchor or counterpart is rejected.

Every batch is all-or-nothing and capped at 50 items. It applies completely or, if any counterpart is rejected, writes nothing at all and reports which IDs failed and why. Because a rejected batch leaves no partial state, simply fix the offending IDs and resend the corrected batch — do not track which pairs applied or write retry logic that re-attaches them individually.

Each policy may have one current document. Choose the document flow based on who can read the file. Neither flow replaces or archives a current document implicitly.

## Human browser management

1. Call `manage_policy_document` to create a short-lived bearer-secret URL for the intended human.
2. Give the URL only to that human. Do not fetch, persist, log, or place it in durable notes.
3. The human opens the URL to upload, download, or archive the current document in a browser.
4. Afterward, call `get_policy` and inspect the document metadata and `upload_status`; issuing a URL does not mean an upload succeeded.

## Trusted-runtime machine upload

Use this flow only when a trusted agent runtime can read the local file and make an HTTP request. File bytes never pass through MCP or the model. Local paths never pass through MCP or the model either.

1. Read the file's name, media type, byte length, and optional SHA-256 in the trusted runtime.
2. Call `prepare_policy_document_upload` with that metadata and the policy ID. Do not pass the local path or file bytes.
3. Execute the returned `PUT` URL from the trusted runtime. Send the returned `Authorization` and `Content-Type` values, set `Content-Length` to the exact declared byte length, and stream the file as the request body. Treat the descriptor and authorization value as bearer secrets and never log them.
4. Poll `get_policy`. A `201` transfer creates the pending document; a matching retry may return `200` with the same document.

If a transfer fails ambiguously, retry with the same descriptor while it is unexpired and the declared metadata is unchanged. Prepare a new upload only after expiry or when the file metadata changes.

Uploads are scanned before they become available, so `upload_status` moves from `pending` through `finalizing` to `uploaded`. A file that fails scanning lands on `contains_virus` or `failed` and cannot be downloaded. If the policy already has a current document, inspect it with `get_policy`; a human must explicitly archive it through browser management before another upload can succeed.
