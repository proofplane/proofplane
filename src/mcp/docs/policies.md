# Policies

Policies record the organization’s rules. Controls define the safeguards that must be proven, and policy-control mappings show which policies govern each control. A policy may map to many controls, a control may map to many policies, and either may exist before a mapping is created.

Start with `list_policies` and `get_policy`. Use `create_policy` or `update_policy` for policy metadata, then use `attach_policy_to_control` and `detach_policy_from_control` to manage relationships without changing the controls themselves. Archive a policy only when it should disappear from active compliance and auditor views.

When you have several relationships to make or break at once, reach for a batch tool instead of one call per pair. A batch fans out from a single anchor to many counterparts, so each direction is its own tool: `attach_policy_to_controls` attaches one policy to many controls, and `attach_control_to_policies` attaches one control to many policies; `detach_policy_from_controls` and `detach_control_from_policies` are the removal halves. There is no both-sides form, and unlike evidence mappings these carry no per-pair rationale. Only active policies can be attached or detached — an archived policy anchor or counterpart is rejected.

Every batch is all-or-nothing and capped at 50 items. It applies completely or, if any counterpart is rejected, writes nothing at all and reports which IDs failed and why. Because a rejected batch leaves no partial state, simply fix the offending IDs and resend the corrected batch — do not track which pairs applied or write retry logic that re-attaches them individually.

Each policy may have one current document. Call `manage_policy_document` to create a short-lived bearer-secret URL for the intended human, who manages the file in a browser. File bytes never pass through MCP or the model. Do not fetch, persist, log, or place the URL in durable notes. After the human finishes, call `get_policy` and inspect the document metadata and `upload_status`; issuing a URL does not mean an upload succeeded.
