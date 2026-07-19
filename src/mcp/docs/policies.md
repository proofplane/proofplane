# Policies

Policies record the organization’s rules. Controls define the safeguards that must be proven, and policy-control mappings show which policies govern each control. A policy may map to many controls, a control may map to many policies, and either may exist before a mapping is created.

Start with `list_policies` and `get_policy`. Use `create_policy` or `update_policy` for policy metadata, then use `attach_policy_to_control` and `detach_policy_from_control` to manage relationships without changing the controls themselves. Archive a policy only when it should disappear from active compliance and auditor views.

Each policy may have one current document. Call `manage_policy_document` to create a short-lived bearer-secret URL for the intended human, who manages the file in a browser. File bytes never pass through MCP or the model. Do not fetch, persist, log, or place the URL in durable notes. After the human finishes, call `get_policy` and inspect the document metadata and `upload_status`; issuing a URL does not mean an upload succeeded.
