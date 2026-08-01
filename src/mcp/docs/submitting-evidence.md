# Submitting Evidence

Start by calling `list_evidence` and reading the selected evidence with
`get_evidence`, especially its `collection_instructions`. Then choose the flow
that matches who can read the file.

## Human browser upload

1. Call `manage_evidence_submissions` with the evidence ID and coverage window.
2. Give the returned short-lived browser URL only to the human who will upload
   the files. It is a bearer secret, so keep it out of durable notes, logs, and
   broadly visible messages.
3. The human opens the URL and uploads one or more files. Each file becomes one
   submission with that coverage window and the connected agent's provenance.
4. Call `list_evidence_submissions` to confirm what arrived.

## Trusted-runtime machine upload

Use this flow only when a trusted agent runtime can read the local file and
make an HTTP request. File bytes and local paths never pass through MCP or the model.

1. Read the local file's name, media type, byte length, and optional SHA-256 in
   the trusted runtime.
2. Call `prepare_evidence_submission_upload` with that metadata, the evidence
   ID, and coverage window. Do not pass the local path or file bytes.
3. Execute the returned `PUT` URL from the trusted runtime. Send the returned
   `Authorization` and `Content-Type` values, set `Content-Length` to the exact
   declared byte length, and stream the file as the request body. Treat the
   descriptor and authorization value as bearer secrets and never log them.
4. Poll `get_evidence_submission` with the returned submission ID. A `201`
   transfer creates the submission; a matching retry may return `200` with the
   same submission and document.

If a transfer fails ambiguously, retry with the same descriptor while it is
unexpired and the declared metadata is unchanged. Prepare a new upload only
after expiry or when the file metadata changes.

Uploads are scanned before they become available, so `upload_status` moves from `pending` through `finalizing` to `uploaded`. A file that fails scanning lands on `contains_virus` or `failed` and cannot be downloaded.

For either flow, do not create new upload authority merely because a submission
is still processing. Inspect its current status first.
