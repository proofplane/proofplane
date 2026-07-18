# Submitting Evidence

1. Call `list_evidence_requests` or `list_due_evidence_requests` to find work. Read each request with `get_evidence_request`, especially its `collection_instructions`, due date, cadence, and freshness window.
2. Collect the requested proof, then call `create_evidence_submission` with the request ID, coverage window, source system, collection method, and a useful summary or description. The submission records the connected agent's provenance.
3. If files are needed, call `manage_evidence_submission_document` with the new submission ID. Give the returned bearer-secret URL only to the human who will manage the documents before it expires; the human uses the browser flow and file bytes never pass through MCP or the model.
4. After the human finishes, call `get_evidence_submission` to inspect document status and detailed metadata. Use `get_latest_evidence_submission` when you need the newest proof for a request.

Do not create a replacement submission merely to retry an document transfer. Issue a fresh document-management URL for the existing submission when the earlier URL expires.
