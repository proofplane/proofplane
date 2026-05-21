# API Fixtures

These request bodies exercise the local evidence request API. The commands below
assume the API is listening on `http://127.0.0.1:3000` with the local seed data
loaded.

The seed binary creates the local workspace at:

```text
00000000-0000-4000-8000-000000000001
```

Create an evidence request:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --data @fixtures/api/evidence-requests/create-quarterly-access-review.json \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```

List requests:

```bash
curl --fail-with-body \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```

List requests that are due at a fixed time:

```bash
curl --fail-with-body \
  'http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests/due?now=2026-05-21T12%3A00%3A00Z'
```

Replace an evidence request after copying its `id` from a create or list
response:

```bash
curl --fail-with-body \
  --request PUT \
  --header 'content-type: application/json' \
  --data @fixtures/api/evidence-requests/replace-paused-vulnerability-scan.json \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests/EVIDENCE_REQUEST_ID
```

Send a validation failure:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --data @fixtures/api/evidence-requests/invalid-evidence-request.json \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```
