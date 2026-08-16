# Proofplane Production Infrastructure

This directory implements the accepted first-production architecture in
[`docs/epics/production-deployment/spec.md`](../../../docs/epics/production-deployment/spec.md).
It does not create the GCP project, attach billing, manage Supabase, store secret
payloads, build images, or change registrar nameservers.

## Phases

The configuration is three Terraform roots that apply in order. Each root is a
complete configuration for its own stage, so none of them is ever applied in a
half-disabled state.

| Order | Root | Creates | State prefix |
| --- | --- | --- | --- |
| 1 | [`01-artifacts/`](./01-artifacts) | Artifact Registry, and the four services that must be enabled before any other root runs | `proofplane/production/artifacts` |
| 2 | [`02-foundation/`](./02-foundation) | APIs, service accounts, secret containers, buckets, topics, the DNS zone, the notification channel, and the budget | `proofplane/production/foundation` |
| 3 | [`03-release/`](./03-release) | Cloud Run workloads, the migration job, Pub/Sub subscriptions, the load balancer, certificates, records, and alert policies | `proofplane/production/release` |

The order is not a preference. Three things force it:

- **Images.** `03-release` takes immutable `@sha256` references as input
  variables, and those digests only exist after a push. The push needs a
  repository, which `01-artifacts` creates. That is why the registry is its own
  root rather than part of the foundation. The push must precede `03-release`.
  It does not have to precede `02-foundation`.
- **Secret payloads.** `03-release` mounts numeric secret versions that an
  operator uploads after `02-foundation` creates the containers, so that no
  plaintext enters Terraform configuration or state.
- **Service enablement.** `01-artifacts` enables
  `cloudresourcemanager.googleapis.com` and `serviceusage.googleapis.com`, which
  every later root needs in order to enable anything of its own. Terraform
  cannot see this dependency, because the two roots have separate state.

`01-artifacts` publishes no output the later roots read. Its coupling to them is
the service enablement above. `03-release` reads `02-foundation` through
`terraform_remote_state`. See
[`03-release/foundation.tf`](./03-release/foundation.tf) for the values it takes.

## Initialize

Create the state bucket manually first — no Terraform root manages it; see
[Terraform State](../../../docs/epics/production-deployment/spec.md#terraform-state)
for its required settings. Every root shares that one bucket under a distinct
prefix. Initialize each root once:

```sh
cd 01-artifacts && make init TF_STATE_BUCKET=YOUR_STATE_BUCKET
cd ../02-foundation && make init TF_STATE_BUCKET=YOUR_STATE_BUCKET
cd ../03-release && make init TF_STATE_BUCKET=YOUR_STATE_BUCKET
```

Terraform caches the backend settings, so later inits in that root need no
arguments. Copy `terraform.tfvars.example` to `terraform.tfvars` in each root
and fill it in.

`03-release` names the foundation state a second time, through `state_bucket`
and `foundation_state_prefix`, because `terraform_remote_state` cannot read the
partial backend config. Both must match what `02-foundation` was initialized
with. If you override `TF_STATE_PREFIX` for `02-foundation`, set
`foundation_state_prefix` to the same value, or `03-release` reads a prefix that
holds no state.

### Make targets

Every root includes [`terraform-root.mk`](./terraform-root.mk) and sets only its
own state prefix and default plan name. The rules track local files, so `init`
re-runs when the backend or provider inputs change, and a saved plan is rebuilt
when a `.tf` file or `terraform.tfvars` is newer:

```sh
make init TF_STATE_BUCKET=YOUR_STATE_BUCKET   # first init in this root only
make plan                                     # writes this root's saved plan
make replan                                   # discard and re-plan
make clean                                    # remove saved plans
```

Make cannot see cloud state, so a saved plan can be stale even when no local
file changed. Use `make replan` before every apply. There is no `apply` target;
run `terraform apply` explicitly against the plan you reviewed. `terraform
import` is also unwrapped.

## Apply order

### 1. Artifacts

```sh
cd 01-artifacts
make replan
terraform apply artifacts.tfplan
```

The repository name defaults to `proofplane` in `us-central1`.
`scripts/push-image.sh` and `scripts/mirror-clamav.sh` build their references
from those same two values, so change `artifact_repository` only together with
those scripts.

### 2. Images

Build, smoke, and push from the repository root. See
[Release Images](../../../docs/runbooks/production-deployment.md#release-images).
Record each immutable `@sha256` reference for `03-release`.

### 3. Foundation

If the `proofplane.app` Cloud DNS zone was created manually, import it before
the first plan instead of allowing Terraform to create a second zone:

```sh
cd 02-foundation
terraform import google_dns_managed_zone.primary \
  projects/YOUR_PROJECT/managedZones/proofplane-app
make replan
terraform apply foundation.tfplan
```

### 4. Secret payloads

Create the payload versions outside Terraform so plaintext never enters
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

### 5. Release

Do not apply this root until every gate in the deployment spec is closed. Set
the three image digests and the two numeric secret versions, then review one
complete plan and apply it once:

```sh
cd 03-release
make replan
terraform apply release.tfplan
```

The beta Cloud Run `run_execution_token` waits for the digest-specific migration
execution to complete successfully before Terraform updates API, MCP, worker, or
dequeuer. A failed migration fails the apply and prevents those dependent
updates.

After apply, perform the smoke checks in the production runbook. Roll back an
application regression by restoring the previous digest and applying again; do
not automatically reverse an expand migration.

## DNS cutover

`02-foundation` creates the Cloud DNS zone and `03-release` adds the certificate
authorization and address records, but an operator must export and reproduce
every Route 53 record and replace the registrar delegation. Use the exact
`cloud_dns_name_servers` output from `02-foundation`. Do not add Google and
Route 53 nameservers together as apex records. Keep Route 53 serving during
propagation and defer DNSSEC until the new delegation is stable.

## Safety notes

- Applying the roots out of order fails rather than corrupts. `03-release`
  cannot plan before `02-foundation` has state to read, and an image cannot be
  pushed before `01-artifacts` creates the repository. Applying `02-foundation`
  before `01-artifacts` fails less clearly, on a service that is not enabled
  yet, so keep to the order above.
- Destroying `02-foundation` while `03-release` holds state removes resources
  the release still references. Destroy in reverse order if you ever must.
- The outputs `02-foundation` publishes are a contract `03-release` depends on.
  Removing one breaks that root's plan.
- Bucket `prevent_destroy`, Cloud Run deletion protection, Pub/Sub deletion
  policies, zone `prevent_destroy`, and the remote backend intentionally make
  wholesale teardown a multi-step manual operation.
- Budgets notify at 50%, 80%, and 100%; they do not cap spend.
- No VPC connector, Cloud NAT, Cloud Armor, uptime check, custom metric scraper,
  or production seed command is created.
