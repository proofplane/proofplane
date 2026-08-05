# Production Deployment Runbook

Use this runbook after the release gates in the
[production deployment spec](../epics/production-deployment/spec.md#release-gates)
are closed. Releases run from an operator workstation; there is no CI/CD path
yet.

## First-Time Preflight

- Confirm the pre-created GCP project is attached to the intended billing
  account and the local identity can administer the planned resources.
- Create the protected GCS state bucket manually (no Terraform root owns it;
  see the [spec](../epics/production-deployment/spec.md#terraform-state)), then
  initialize the production root with
  `make init TF_STATE_BUCKET=YOUR_STATE_BUCKET`.
- Import the manually created `proofplane.app` Cloud DNS zone and confirm its
  nameservers match the intended registrar delegation.
- Export every Route 53 record, reproduce it in Cloud DNS, reduce TTLs before
  cutover, and query the Cloud DNS nameservers directly before changing the
  registrar.
- Confirm Supabase SSL enforcement and daily backups are active. The accepted
  launch database RPO is approximately 24 hours; PITR is deferred.
- Verify runtime traffic uses the Supavisor transaction pooler on port 6543 and
  migrations use the separate direct verified-TLS credential.
- Upload the complete production YAML and migration database URL as separate
  Secret Manager versions. Record numeric versions, not aliases.
- Confirm the latest validated ClamAV snapshot is less than 24 hours old.

## Build And Plan

1. Start from a clean intended checkout and run the repository's full checks.
2. Build the Linux production image and smoke every packaged command locally.
3. Push Proofplane and the pinned mirrored ClamAV images to the regional
   repository. Resolve and record immutable `@sha256` references.
4. Review Artifact Analysis findings. Scanning is advisory at launch, but known
   critical findings require an explicit operator decision before proceeding.
5. Update only digest and numeric secret-version inputs. Run `make replan`
   (or `terraform plan -out …`) and save the reviewed plan.
6. Reject a plan that contains mutable tags, unexpected replacement/deletion,
   public worker access, runtime Pub/Sub administration, seed execution, or an
   unpinned secret version.

## Apply And Verify

Apply the saved plan once. Terraform executes and waits for
`proofplane-migrate` before updating serving workloads.

After a successful apply:

- Confirm the migration execution succeeded once for the selected digest.
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
- **Migration succeeds but smoke checks fail:** restore the previous application
  digest and apply. Expand migrations remain in place and must be compatible
  with the prior revision.
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

