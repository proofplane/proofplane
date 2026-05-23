# API Fixtures

These request bodies exercise the local evidence request API. The commands below
assume the API is listening on `http://127.0.0.1:3000` with the local seed data
loaded.

The seed binary creates two local workspaces:

```text
authorized workspace:   00000000-0000-4000-8000-000000000001
unauthorized workspace: 00000000-0000-4000-8000-000000000002
```

The seeded `system-actor` has SpiceDB membership only in the authorized
workspace. The unauthorized workspace exists for local cross-workspace denial
checks and has no seeded Evidence Requests.

Each seed run rotates the local API key for the authenticated `system-actor`.
Use the key printed by the latest `seed` run:

```text
local system actor API key (rotated by this seed run): proof-dev-...
```

Set the key shown in this output to `PROOFPLANE_API_KEY`.

Create an evidence request:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/create-quarterly-access-review.json \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```

List requests:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```

List requests that are due at a fixed time:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  'http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests/due?now=2026-05-21T12%3A00%3A00Z'
```

Replace an evidence request after copying its `id` from a create or list
response:

```bash
curl --fail-with-body \
  --request PUT \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/replace-paused-vulnerability-scan.json \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests/EVIDENCE_REQUEST_ID
```

Send a validation failure:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/invalid-evidence-request.json \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000001/evidence-requests
```

Confirm cross-workspace authorization denial. This should return `404` for a
valid `system-actor` API key:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  http://127.0.0.1:3000/workspaces/00000000-0000-4000-8000-000000000002/evidence-requests
```
