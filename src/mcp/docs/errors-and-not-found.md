# Errors and Not Found

- `validation_failed` means one or more arguments are missing or invalid. Read every `field_issues` entry, correct the named fields, and retry only after changing the request.
- `not_found` is deliberately ambiguous: the object may not exist, may not be visible to the connection, or the connection may lack the required permission. Recheck IDs and the intended operation; do not probe neighboring IDs or claim which condition occurred.
- Conflict codes such as `control_code_taken` and `evidence_request_control_mapping_exists` mean the requested state already conflicts with stored state. Read the object or mapping, then choose whether to use it, replace it, or stop.
- `dependency_failed` means a server dependency failed while handling an otherwise valid call. Retry with bounded backoff; if repeated attempts fail, report the operation and stable identifiers without exposing bearer-secret URLs.

Do not retry validation or conflict failures unchanged. Retry transient dependency failures, and request a new document URL when only that short-lived grant has expired.
