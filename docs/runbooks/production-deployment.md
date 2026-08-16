# Production Deployment Runbook

Use this runbook after the release gates in the
[production deployment spec](../epics/production-deployment/spec.md#release-gates)
are closed. Releases run from an operator workstation; there is no CI/CD path
yet.

## First-Time Preflight

These steps run once, in this order. Several of them create a thing a later step
needs, so the order is not a suggestion.

1. Confirm the pre-created GCP project is attached to the intended billing
   account and the local identity can administer the planned resources.
2. Create the protected GCS state bucket manually (no Terraform root owns it;
   see the [spec](../epics/production-deployment/spec.md#terraform-state)), then
   initialize each of the three phase roots in `infra/gcp/production/` with
   `make init TF_STATE_BUCKET=YOUR_STATE_BUCKET`. They share the bucket under
   distinct prefixes.
3. Apply `01-artifacts`. It creates the Artifact Registry repository, and it
   enables the services every later root needs, so nothing else proceeds without
   it.
4. Build, smoke, and push the release and ClamAV images. See
   [Release Images](#release-images).
5. Apply `02-foundation`. It creates the service accounts, secret containers,
   buckets, topics, DNS zone, notification channel, and budget. Import the
   manually created `proofplane.app` Cloud DNS zone first, so Terraform adopts
   it rather than creating a second zone, and confirm its nameservers match the
   intended registrar delegation.
6. Upload the complete production YAML and migration database URL as separate
   Secret Manager versions, into the containers step 5 created. Record numeric
   versions, not aliases.
7. Apply `03-release`, following [Build And Plan](#build-and-plan).

Confirm the following before the release apply. None of them depend on the
phase order:

- Export every Route 53 record, reproduce it in Cloud DNS, reduce TTLs before
  cutover, and query the Cloud DNS nameservers directly before changing the
  registrar.
- Confirm Supabase SSL enforcement and daily backups are active. The accepted
  launch database RPO is approximately 24 hours. PITR is deferred.
- Verify runtime traffic uses the Supavisor transaction pooler on port 6543 and
  migrations use the separate direct verified-TLS credential.
- Confirm the latest validated ClamAV snapshot is less than 24 hours old.

## Build And Plan

Terraform applies in three phases, and the image push sits between the first and
the last. See
[`infra/gcp/production/README.md`](../../infra/gcp/production/README.md) for the
full order and why it is fixed.

1. Start from a clean intended checkout and run the repository's full checks.
2. Confirm `01-artifacts` is applied, so the regional repository exists.
3. Build the Linux production image and smoke every packaged command locally.
   See [Release Images](#release-images).
4. Push Proofplane and the pinned mirrored ClamAV images to the regional
   repository. Resolve and record immutable `@sha256` references.
5. Review Artifact Analysis findings. Scanning is advisory at launch, but known
   critical findings require an explicit operator decision before proceeding.
6. Apply `02-foundation` if its inputs changed, then create or rotate any secret
   payload version the release needs. On a first deployment this apply is not
   optional; see [First-Time Preflight](#first-time-preflight).
7. In `03-release`, update only digest and numeric secret-version inputs. Run
   `make replan` (or `terraform plan -out …`) and save the reviewed plan.
8. Reject a plan that contains mutable tags, unexpected replacement/deletion,
   public worker access, runtime Pub/Sub administration, seed execution, or an
   unpinned secret version.

## Release Images

One image carries every production command. Each Cloud Run resource selects a
command by absolute path, so the image ships `api`, `mcp`, `worker`, `dequeuer`,
and `migrate` under `/usr/local/bin/`. It does not ship `seed`.

| Command | Result |
| --- | --- |
| `make image` | Builds the `linux/amd64` image and records its reference in `.local/image-ref`. |
| `make image-smoke` | Runs every packaged command and checks the platform, the runtime user, and the certificate bundle. |
| `make image-push` | Runs the smoke checks, pushes to Artifact Registry, and prints the `@sha256` reference. |
| `make clamav-mirror` | Copies the pinned ClamAV image into the same repository and prints its `@sha256` reference. |

`make image-push` and `make clamav-mirror` need `PROOFPLANE_PROJECT_ID`. Both
print one immutable reference on stdout. Copy that reference into
`03-release/tfvars/production.tfvars`. Terraform rejects a mutable tag.

`make image` refuses a worktree that has uncommitted changes. The digest is the
only record of what a release contains, so an image built from uncommitted work
cannot be reproduced later.

The first build is slow. It compiles Rust under emulation. Later builds reuse
the cargo cache mounts.

The mirrored ClamAV image is a pinned base, not a deployable sidecar. Both
`clamav_image_digest` and `clamav_updater_image_digest` need derived images that
[#121](https://github.com/proofplane/proofplane/issues/121) owns.

### Image Retention

`infra/gcp/production/01-artifacts/artifacts.tf` owns retention, and its cleanup
policies are live rather than a dry run. Two policies apply:

- `keep-recent-releases` keeps the 20 most recent versions.
- `delete-untagged-after-30-days` deletes an untagged version after 30 days.

The delete policy matches only untagged versions, so a tag is what protects a
digest an operator may still need. `make image-push` and `make clamav-mirror`
both push a tag for that reason. Never push a release image without one.

A deleted digest cannot be redeployed, and a release cannot be reached without
it. This matters more after #157: a runtime accepts work only when its own
embedded migration history matches the database, so recovery needs an image
built against the history in question.

No policy deletes a tagged version today, so tagged releases accumulate. The
spec asks to "retain a bounded number of old release images", which the current
policies do not achieve for tagged images.
[#118](https://github.com/proofplane/proofplane/issues/118) owns
`artifacts.tf`, and a bounded policy belongs there. Until then, delete old
release tags by hand when the repository grows.

`01-artifacts` owns nothing that serves traffic, so a retention change plans and
applies without touching a running workload.

### Rollback Digests

Record two digests after every release: the digest now deployed, and the digest
deployed before it. `terraform output deployed_application_digest` reports the
first one.

A digest rollback is safe only when the target image embeds the same migration
history the database now holds. That is true when the release you undo applied
no migration. Set `app_image_digest` to the previous digest, plan, and apply.

Do not roll the digest back when the release did apply a migration. Every
runtime checks its own embedded history against the database before it accepts
work, so the older image rejects the newer history and refuses to start. Roll
forward instead, with a corrected binary that embeds the applied history. See
[Failure And Rollback](#failure-and-rollback).

Cloud Run derives the migration execution token from the first 12 characters of
the digest. See `infra/gcp/production/03-release/locals.tf`. The token is the
suffix of the execution name, so a previous digest names an execution that
already exists. Expect a digest rollback to start no new migration execution.
Confirm this during the first rehearsed rollback, which
[#124](https://github.com/proofplane/proofplane/issues/124) owns.

## Apply And Verify

Apply the `03-release` plan once. Terraform executes and waits for
`proofplane-migrate` before updating serving workloads.

After a successful apply:

- Confirm the migration execution succeeded once for the selected digest.
- Confirm every runtime passed its read-only schema-history check before it
  accepted work.
- Confirm API and MCP revisions use that digest, have min instances zero, and
  reject direct internet access to their default URLs.
- Confirm the dequeuer worker pool has exactly one instance.
- Confirm the worker has min zero/max one, concurrency four, clamd is ready,
  and an unsigned request to its `run.app` URL is rejected.
- Publish a non-destructive test message and verify authenticated delivery,
  processing, and acknowledgment without a dead-letter event.
- Verify `https://api.proofplane.app` and `https://mcp.proofplane.app` through
  the load balancer, valid certificates, HTTP-to-HTTPS redirects, expected
  version output, and a non-destructive application flow.
- Confirm alert delivery and the $100 budget thresholds are visible.

Do not add continuous public uptime probes; they defeat the accepted
scale-to-zero behavior.

## DNS Delegation

At the registrar, replace the Route 53 nameservers with the complete Cloud DNS
set from Terraform output. This is registrar-level delegation even if the
registrar remains AWS. Do not edit Route 53's apex NS record to mix providers.

Keep Route 53 intact while TTLs expire. Check several public resolvers for the
Google nameservers, all preserved zone records, API/MCP addresses, and valid TLS.
Only then remove the old hosted zone. If records are missing, restore the old
registrar delegation while Route 53 remains available. Enable DNSSEC only in a
later, separately verified change.

## Failure And Rollback

- **Migration fails:** Terraform stops before dependent revisions update.
  Inspect job logs, correct the forward migration, publish a new digest if code
  changed, and re-plan. Never seed production.
- **Migration succeeds but smoke checks fail:** do not restore the previous
  application digest. Once a newer history entry exists, an older image rejects
  it on startup even when the migration is additive. Keep already-running old
  revisions available while diagnosing when possible, then publish and apply a
  corrected binary that embeds the exact applied history. Do not reverse an
  expand migration automatically.
- **ClamAV update fails:** the updater must leave the last-good pointer intact.
  After two failures, investigate CDN access, image version, validation logs,
  and bucket IAM. Workers fail closed after 24 hours of staleness.
- **Dead-letter message appears:** inspect the persistent 31-day pull
  subscription, preserve the message, correct the idempotent handler or data,
  and replay deliberately. Do not purge the subscription as diagnosis.
- **Evidence deleted accidentally:** recover it from the bucket's 30-day soft
  delete window. Database daily backup recovery is separate and may lose up to
  roughly 24 hours.
- **Secret rotation:** add a new secret version outside Terraform, update its
  numeric version input, review the new revisions, and apply. Disable old
  versions only after rollback no longer depends on them.
