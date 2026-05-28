# API Fixtures

These request bodies exercise the local API against the seed data created by
`make seed`.

## Start Local Services

From the repository root:

```bash
make up
make health
make authz-schema
make seed
```

Each seed run rotates the local API key for `system-actor`. Copy the key printed
by the latest seed run:

```text
local system actor API key (rotated by this seed run): proof-dev-...
```

In a separate terminal, start the API:

```bash
make api
```

Set common shell variables:

```bash
export BASE_URL=http://127.0.0.1:3000
export WORKSPACE_ID=00000000-0000-4000-8000-000000000001
export UNAUTHORIZED_WORKSPACE_ID=00000000-0000-4000-8000-000000000002
export SOC2_FRAMEWORK_ID=136bfa09-f431-589c-ba4f-0176ad981a39
export SEEDED_ACCESS_CONTROL_ID=25559395-28b2-5e5f-9fae-4a68d4386d5e
export PROOFPLANE_API_KEY=proof-dev-replace-with-latest-seed-output
```

The authorized workspace has SpiceDB membership for `system-actor`. The
unauthorized workspace exists for local denial checks.

## Frameworks

List seeded frameworks:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/frameworks"
```

List SOC 2 requirements:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/frameworks/$SOC2_FRAMEWORK_ID/requirements"
```

## Controls

List seeded controls:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/controls"
```

Create a fixture control:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/controls/create-fixture-access-control.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/controls"
```

To replace the control, copy its `id` from the create or list response:

```bash
export CONTROL_ID=replace-with-control-id

curl --fail-with-body \
  --request PUT \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/controls/replace-fixture-access-control.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/controls/$CONTROL_ID"
```

Send a control validation failure. This should return `400`:

```bash
curl --include \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/controls/invalid-control.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/controls"
```

## Evidence Requests

Create an evidence request:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/create-quarterly-access-review.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests"
```

List requests:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests"
```

List requests due at a fixed time:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/due?now=2026-05-21T12%3A00%3A00Z"
```

Replace an evidence request after copying its `id` from a create or list
response:

```bash
export EVIDENCE_REQUEST_ID=replace-with-evidence-request-id

curl --fail-with-body \
  --request PUT \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/replace-paused-vulnerability-scan.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID"
```

Send an evidence request validation failure. This should return `400`:

```bash
curl --include \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/invalid-evidence-request.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests"
```

## Control Mappings

Create a mapping from an evidence request to the seeded `PP-AC-01` control.
Set `EVIDENCE_REQUEST_ID` to an existing evidence request ID first:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/control-mappings/create-seeded-access-review-mapping.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings"
```

List mappings for the evidence request:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings"
```

Delete the mapping:

```bash
curl --fail-with-body \
  --request DELETE \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings/$SEEDED_ACCESS_CONTROL_ID"
```

Send a mapping validation failure. This should return `400`:

```bash
curl --include \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/control-mappings/invalid-mapping.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings"
```

## Authorization Checks

Confirm cross-workspace authorization denial. This should return `404` for a
valid `system-actor` API key:

```bash
curl --include \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$UNAUTHORIZED_WORKSPACE_ID/evidence-requests"
```

Evidence request `DELETE` is intentionally unsupported. This should return
`405`:

```bash
curl --include \
  --request DELETE \
  --header 'x-proofplane-actor-id: system-actor' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID"
```

If fixture creates conflict because you already ran them, either use the list
response IDs for replace/delete calls or reset local state with:

```bash
make reset-local
make health
make authz-schema
make seed
```
