# Submitting Evidence

1. Call `list_evidence` to find work, and read a piece of evidence with `get_evidence` — especially its `collection_instructions`.
2. Call `manage_evidence_submissions` with the evidence ID and the coverage window the proof covers. It returns a short-lived browser URL. Give it only to the human who will upload the files; the URL is a bearer secret, so keep it out of durable notes, logs, and broadly visible messages.
3. The human opens the URL and uploads one or more files. Each file becomes one submission stamped with that coverage window and the connected agent's provenance. File bytes never pass through MCP or the model.
4. Call `list_evidence_submissions` to confirm what arrived, and inspect each submission's document metadata and `upload_status`. Do not assume that issuing a URL means the upload succeeded. Use `get_evidence_submission` when you already hold a submission ID.

Uploads are scanned before they become available, so `upload_status` moves from `pending` through `finalizing` to `uploaded`. A file that fails scanning lands on `contains_virus` or `failed` and cannot be downloaded.

Do not issue a new link merely to retry a transfer that is still processing. If a link expires, request a fresh one for the same evidence and coverage window — it will still list the files already uploaded for that period.
