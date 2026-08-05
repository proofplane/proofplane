# Proofplane Production Infrastructure

This Terraform root implements the accepted first-production architecture in
[`docs/epics/production-deployment/spec.md`](../../../docs/epics/production-deployment/spec.md).
It does not create the GCP project, attach billing, manage Supabase, store secret
payloads, build images, or change registrar nameservers.

## Initialize

Create the state bucket manually first — no Terraform root manages it; see
[Terraform State](../../../docs/epics/production-deployment/spec.md#terraform-state)
for its required settings. Then initialize this root with a distinct prefix:

```sh
terraform init \
  -backend-config="bucket=YOUR_STATE_BUCKET" \
  -backend-config="prefix=proofplane/production"
cp terraform.tfvars.example terraform.tfvars
```

If the `proofplane.app` Cloud DNS zone was created manually, import it before
the first plan instead of allowing Terraform to create a second zone:

```sh
terraform import google_dns_managed_zone.primary \
  projects/YOUR_PROJECT/managedZones/proofplane-app
```

### Make Targets

`make` wraps the two routine commands from this directory. It tracks the local
`.tf` files, so `init` re-runs only when the backend or provider inputs change,
and a saved plan is rebuilt only when a `.tf` file or `terraform.tfvars` is
newer:

```sh
make init TF_STATE_BUCKET=YOUR_STATE_BUCKET   # first init only; prefix defaults
                                              # to proofplane/production
make plan                                     # writes production.tfplan
make plan PLAN=foundation.tfplan              # name the saved plan per phase
make replan                                   # discard and re-plan
```

Make cannot see cloud state, so a saved plan can be stale even when no local
file changed. Use `make replan` before every apply. There is no `apply` target;
run `terraform apply` explicitly against the plan you reviewed. `terraform
import` is also unwrapped.

## Foundation Apply

Keep `release_enabled = false`. Review and apply the plan to create shared
APIs, service accounts, secret containers, buckets, topics, Artifact Registry,
Cloud DNS, the notification channel, and the budget:

```sh
terraform plan -out foundation.tfplan   # or: make plan PLAN=foundation.tfplan
terraform apply foundation.tfplan
```

Create secret payload versions outside Terraform so plaintext never enters
configuration or state:

```sh
gcloud secrets versions add proofplane-production-config \
  --project YOUR_PROJECT \
  --data-file /absolute/path/to/production-config.yaml
gcloud secrets versions add proofplane-production-migration-database-url \
  --project YOUR_PROJECT \
  --data-file /absolute/path/to/migration-database-url.txt
```

Secret Manager grants access at secret-container scope. Each Cloud Run revision
mounts the numeric version selected in `terraform.tfvars`; never use `latest`.

## Release Apply

Do not enable a release until every gate in the deployment spec is closed.
Build and push the Proofplane, ClamAV, and ClamAV updater images locally, resolve
their immutable `@sha256` references, then set:

- `release_enabled = true`;
- all three image digest variables;
- `runtime_config_secret_version`;
- `migration_database_secret_version`.

Review one complete plan and apply it once. The beta Cloud Run
`run_execution_token` waits for the digest-specific migration execution to
complete successfully before Terraform updates API, MCP, worker, or dequeuer.
A failed migration fails the apply and prevents those dependent updates.

```sh
terraform plan -out release.tfplan   # or: make plan PLAN=release.tfplan
terraform apply release.tfplan
```

After apply, perform the smoke checks in the production runbook. Roll back an
application regression by restoring the previous digest and applying again;
do not automatically reverse an expand migration.

## DNS Cutover

Terraform creates the Cloud DNS zone and certificate authorization records, but
an operator must export/reproduce every Route 53 record and replace the
registrar delegation. Use the exact `cloud_dns_name_servers` output. Do not add
Google and Route 53 nameservers together as apex records. Keep Route 53 serving
during propagation and defer DNSSEC until the new delegation is stable.

## Safety Notes

- `release_enabled` is a bootstrap guard, not a feature switch. Changing it
  back to false after launch proposes removal of protected production resources
  and should fail because deletion protections are enabled.
- Bucket `prevent_destroy`, Cloud Run deletion protection, Pub/Sub deletion
  policies, and the remote backend intentionally make wholesale teardown a
  multi-step manual operation.
- Budgets notify at 50%, 80%, and 100%; they do not cap spend.
- No VPC connector, Cloud NAT, Cloud Armor, uptime check, custom metric scraper,
  or production seed command is created.

