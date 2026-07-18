# Documents

`manage_evidence_submission_document` returns a short-lived browser URL for a human to upload or download files attached to an evidence submission. The URL is a bearer secret: anyone holding it can use the grant until it expires, so share it only with the intended human and never place it in durable notes, logs, or broadly visible messages.

The human opens the URL in a browser and transfers the file there. File bytes never pass through MCP or the model. If the URL expires, request a new one for the same submission instead of reusing or reconstructing it.

After the browser flow completes, call `get_evidence_submission` and inspect each document's filename, checksums, size, content type, and `upload_status`. Do not assume that issuing a URL means the upload succeeded.
