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

Each seed run rotates the local API key for `00000000-0000-4000-8000-000000000106`. Copy the key printed
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
export ACTOR_ID=00000000-0000-4000-8000-000000000106
export PROOFPLANE_API_KEY=proof-dev-replace-with-latest-seed-output
```

The authorized workspace has SpiceDB membership for `00000000-0000-4000-8000-000000000106`. The
unauthorized workspace exists for local denial checks.

## Frameworks

List seeded frameworks:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/frameworks"
```

List SOC 2 requirements:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/frameworks/$SOC2_FRAMEWORK_ID/requirements"
```

## Controls

List seeded controls:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/controls"
```

Create a fixture control:

```bash
curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
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
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/controls/replace-fixture-access-control.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/controls/$CONTROL_ID"
```

Send a control validation failure. This should return `400`:

```bash
curl --include \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
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
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/create-quarterly-access-review.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests"
```

List requests:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests"
```

List requests due at a fixed time:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
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
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-requests/replace-paused-vulnerability-scan.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID"
```

Send an evidence request validation failure. This should return `400`:

```bash
curl --include \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
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
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/control-mappings/create-seeded-access-review-mapping.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings"
```

List mappings for the evidence request:

```bash
curl --fail-with-body \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings"
```

Delete the mapping:

```bash
curl --fail-with-body \
  --request DELETE \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings/$SEEDED_ACCESS_CONTROL_ID"
```

Send a mapping validation failure. This should return `400`:

```bash
curl --include \
  --request POST \
  --header 'content-type: application/json' \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/control-mappings/invalid-mapping.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/control-mappings"
```

## Submissions

```bash
EVIDENCE_REQUEST_ID=$(curl --fail-with-body \
  --header "x-proofplane-actor-id: $ACTOR_ID" \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests" | jq -rc '.[] | select(.title=="Monthly vulnerability scan") | .id')

SUB_JSON=$(curl --fail-with-body \
  --request POST \
  --header 'content-type: application/json' \
  --header "x-proofplane-actor-id: $ACTOR_ID" \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --data @fixtures/api/evidence-submissions/monthly-vulnerability-scan.json \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-requests/$EVIDENCE_REQUEST_ID/submissions")

export SUBMISSION_ID=$(jq -r .id <<< "$SUB_JSON")

DIGEST=$(scripts/content-digest-crc32c.py fixtures/api/evidence-submissions/vuln-scanner-results.txt)
printf 'Content-Digest: %s\n' "$DIGEST" > /tmp/proofplane-part-headers.txt

ATTACH_JSON=$(curl --fail-with-body \
  --request POST \
  --header "x-request-id: $(uuidgen)" \
  --header "x-proofplane-actor-id: $ACTOR_ID" \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --form "file=@fixtures/api/evidence-submissions/vuln-scanner-results.txt;type=text/plain;headers=@/tmp/proofplane-part-headers.txt" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-submissions/$SUBMISSION_ID/attachments")

export ATTACHMENT_ID=$(jq -r '.attachment.id' <<< "$ATTACH_JSON")

echo "$ATTACH_JSON" | jq .
```

After the dequeuer and worker process the scan and finalization requests,
confirm the attachment reports `upload_status` as `uploaded`:

```bash
curl --fail-with-body \
  --header "x-proofplane-actor-id: $ACTOR_ID" \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-submissions/$SUBMISSION_ID" |
  jq --arg attachment_id "$ATTACHMENT_ID" \
    '.attachments[] | select(.id == $attachment_id)'
```

Issue a five-minute download grant for the uploaded attachment:

```bash
GRANT_JSON=$(curl --fail-with-body \
  --request POST \
  --header "x-proofplane-actor-id: $ACTOR_ID" \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-submissions/$SUBMISSION_ID/attachments/$ATTACHMENT_ID/download-grants")

echo "$GRANT_JSON" | jq .
export ATTACHMENT_DOWNLOAD_URL=$(jq -r .url <<< "$GRANT_JSON")
```

Redeem the grant without API credentials and save the attachment:

```bash
curl --fail-with-body \
  --remote-header-name \
  --remote-name \
  "$ATTACHMENT_DOWNLOAD_URL"
```

The URL is reusable until its five-minute expiry. Treat its `token` query
parameter as a bearer secret and do not include it in logs or analytics.

### Malicious Scan Fixture

`fixtures/api/evidence-submissions/eicar.com.txt` contains the standard EICAR
anti-malware test signature. It is harmless text, not executable malware, but
ClamAV and other antivirus products intentionally detect it as malicious. Host
antivirus software may quarantine the fixture.

With `SUBMISSION_ID` set as above, upload it using the same attachment flow:

```bash
EICAR_FIXTURE=fixtures/api/evidence-submissions/eicar.com.txt
DIGEST=$(scripts/content-digest-crc32c.py "$EICAR_FIXTURE")
printf 'Content-Digest: %s\n' "$DIGEST" > /tmp/proofplane-part-headers.txt

curl --fail-with-body \
  --request POST \
  --header "x-request-id: $(uuidgen)" \
  --header "x-proofplane-actor-id: $ACTOR_ID" \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  --form "file=@$EICAR_FIXTURE;type=text/plain;headers=@/tmp/proofplane-part-headers.txt" \
  "$BASE_URL/workspaces/$WORKSPACE_ID/evidence-submissions/$SUBMISSION_ID/attachments" |
  jq .
```

After the dequeuer and worker process the scan request, submission details
should report the attachment's `upload_status` as `contains_virus`.

## Authorization Checks

Confirm cross-workspace authorization denial. This should return `404` for a
valid `00000000-0000-4000-8000-000000000106` API key:

```bash
curl --include \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
  --header "x-proofplane-api-key: $PROOFPLANE_API_KEY" \
  "$BASE_URL/workspaces/$UNAUTHORIZED_WORKSPACE_ID/evidence-requests"
```

Evidence request `DELETE` is intentionally unsupported. This should return
`405`:

```bash
curl --include \
  --request DELETE \
  --header 'x-proofplane-actor-id: 00000000-0000-4000-8000-000000000106' \
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
