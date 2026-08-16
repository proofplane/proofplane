# Production Deployment Spec

## Goal

Deploy Proofplane for the first time in a dedicated, pre-created GCP project in
`us-central1`. Terraform owns production infrastructure, a local operator owns
build and apply commands, and the application is not considered deployable
until every explicit release gate is closed.

This spec implements
[ADR 0002](../../adr/0002-deploy-production-on-cloud-run.md). It does not
authorize application changes while the aggregate refactor is in progress.

## Non-Goals

- Creating the GCP project, attaching billing, or managing the organization.
- Managing the existing Supabase project.
- CI/CD or unattended release promotion.
- Cloud Armor, static outbound IPs, Supabase network restrictions, or PITR.
- Central Prometheus/OpenTelemetry collection.
- A public website at `proofplane.app` or `www.proofplane.app`.
- Production seed data.

## Release Gates

Terraform may be developed before these gates close, but production traffic
must not be enabled until all are verified:

| Gate | Required outcome |
| --- | --- |
| Native GCS | The runtime object-store adapter streams all required operations through GCS using ADC. |
| Native Pub/Sub | The dequeuer publishes through Google Pub/Sub without requiring `PUBSUB_EMULATOR_HOST` and does not provision topics or subscriptions. |
| Database TLS | Runtime and migration connections verify the server certificate and hostname; Supabase SSL enforcement remains enabled. |
| Transaction pooling | Runtime queries issue unnamed statements and pass integration tests against a transaction pooler. Met locally against PgBouncer in transaction mode; still unverified against Supavisor on 6543. |
| Dedicated migrations | A `proofplane-migrate` command runs migrations only, uses a separate direct credential, and never seeds data. |
| Startup behavior | API, MCP, worker, and dequeuer never run migrations during startup and accept work only when database history exactly matches the migrations embedded in the binary. |
| Compatible migrations | Schema changes follow expand/contract rules, use a short lock timeout, and remain compatible with old and new revisions. |
| Stateless MCP | `rmcp` streamable HTTP uses `stateful_mode = false` and direct JSON responses; no in-memory session affinity is required. |
| Bounded pools | Pool sizes, acquisition timeouts, and idle timeouts are configurable. Initial sizes are API 10, MCP 10, worker 6, and dequeuer 2. |
| Production image | One image contains API, MCP, worker, dequeuer, and migrate commands and passes local build/smoke validation. |

The existing seed command is never executed in production.

## Topology

```text
Internet
   |
   v
Global external Application Load Balancer
   |-- api.proofplane.app --> Cloud Run API (0..3)
   `-- mcp.proofplane.app --> Cloud Run MCP (0..3, stateless)

Cloud Run dequeuer worker pool (exactly 1)
   |-- polls Supabase outbox through transaction pooler
   `-- publishes --> Pub/Sub topic
                         |
                         v  authenticated OIDC push
                    private Cloud Run worker (0..1, concurrency 4)
                         |-- application container
                         `-- clamd sidecar

Cloud Scheduler --> ClamAV update job --> private definitions bucket
Terraform apply --> migration job --> serving workload revisions

All durable document objects --> private evidence bucket
All runtime database traffic --> Supavisor transaction pooler
Migration traffic --> verified-TLS direct Supabase endpoint
```

## Project, Region, And Providers

The project already exists and is supplied as a Terraform variable. Terraform
enables only the APIs it consumes. All regional resources use `us-central1`.
Pin compatible `google` and `google-beta` provider versions because Cloud Run
worker pools and job execution tokens may require beta fields.

Use Google-managed encryption unless a later compliance decision requires
CMEK. Apply labels for application, environment, component, and Terraform
ownership where a resource supports them.

## Terraform State

State goes in a bucket the operator creates manually, before any production
root is initialized. No Terraform root manages that bucket _(revised during
ticket 003 — see [Revisions](#revisions))_.

The operator creates the bucket with:

- a globally unique operator-supplied name;
- `us-east1` location;
- uniform bucket-level access and public-access prevention;
- object versioning;
- Google-managed encryption;
- access for the approved Terraform operators only.

`us-east1` is used as the region because it's the nearest region to the
operators and that's where the state is going to be read from.

Production Terraform is three roots that apply in order, and each declares a
partial `gcs` backend. They share the one bucket under distinct prefixes,
supplied at `terraform init`:

| Root | Prefix |
| --- | --- |
| `infra/gcp/production/01-artifacts` | `proofplane/production/artifacts` |
| `infra/gcp/production/02-foundation` | `proofplane/production/foundation` |
| `infra/gcp/production/03-release` | `proofplane/production/release` |

```sh
make init TF_STATE_BUCKET=YOUR_STATE_BUCKET   # in each root, once
```

Terraform caches the backend configuration in `.terraform/terraform.tfstate`,
so later inits need no arguments. Because the bucket is not a Terraform
resource, its protection is whatever the operator configured at creation;
`prevent_destroy` does not apply to it.

`03-release` reads `02-foundation` through `terraform_remote_state`, so it also
takes the bucket name as an ordinary variable. That data source cannot read the
partial backend configuration.

Secrets and secret-version payloads never appear in Terraform configuration or
state.

## Artifact Registry And Images

Create one regional Docker repository. A local build produces a Linux image,
pushes a content-addressed artifact, resolves its `sha256` digest, and supplies
that digest to Terraform. Never deploy mutable tags.

The repository is its own Terraform root, `01-artifacts`, and applies before
every other one. A digest cannot exist until an image is pushed, and an image
cannot be pushed until the repository exists, so the repository cannot share a
root with anything that consumes a digest.

The Proofplane image contains all production commands. Each Cloud Run resource
overrides the command instead of building a process-specific image. Mirror a
pinned official ClamAV image into the same regional repository so production
does not depend on Docker Hub availability or mutable tags.

The root `Dockerfile` builds that image in two stages:

| Property | Value | Reason |
| --- | --- | --- |
| Platform | `linux/amd64`, pinned in the `FROM` lines | Cloud Run accepts nothing else, and the operator workstation is usually arm64. |
| Builder | `rust:1.95-bookworm` plus `cmake` | The image already carries `pkg-config` and `libssl-dev` for `openssl-sys`. `aws-lc-sys` builds through `cmake`, which it does not carry. |
| Runtime | `debian:bookworm-slim` plus `ca-certificates` and `libssl3` | `openssl-sys` links dynamically, so a `scratch` or static-distroless runtime is not available. `reqwest` verifies TLS through the platform certificate store. |
| User | UID 10001 | Nothing writes to the filesystem. Configuration arrives as a read-only secret mount, and evidence goes to object storage. |
| Commands | `/usr/local/bin/{api,mcp,worker,dequeuer,migrate}` | `run.tf` selects each command by absolute path and supplies no arguments. |
| Default command | None | Every Cloud Run resource sets its command. A default could only hide a missing override. |

The image does not contain `seed`. Leaving the command out is the surest way to
honor the gate that rejects seed execution in production.

`scripts/smoke-image.sh` validates a built image before a push. It checks the
platform, the declared and observed user, the certificate bundle, and the
absence of `seed`. It then runs each of the five commands with an empty
environment and a read-only root filesystem, and requires each one to exit 1
with its own documented credential message. That is what proves a command
executes: the ELF loads, the dynamic `libssl` link resolves, and `main` runs.
The read-only filesystem proves the other claim in the table above, that no
command needs to write anything to start.

`run.tf` consumes two ClamAV digests rather than one. `clamav_image_digest` is
the worker sidecar and `clamav_updater_image_digest` is the update job. Neither
is stock upstream: the sidecar copies the last-good snapshot from GCS and
disables `freshclam`, and the updater needs its own entrypoint. The mirror is
the pinned base that both derived images start from.

Enable Artifact Analysis automatic scanning. Findings are advisory initially;
no severity threshold blocks a local deployment until an exception workflow
exists. Retain a bounded number of old release images through repository
cleanup policy while preserving the currently deployed and immediately prior
digests.

## Configuration And Secrets

Store one complete production YAML document in Secret Manager. Terraform owns
the secret container, IAM, and pinned version reference, but a local operator
creates the payload version outside Terraform. Mount the selected version
read-only and set `PROOFPLANE_CONFIG` to the mounted path.

API, MCP, worker, and dequeuer deliberately share this complete configuration
for the first deployment. Their distinct service accounts still restrict GCP
resource access. A separate secret supplies the migration-only direct database
credential to the migration job. Secret aliases such as `latest` are forbidden
in revisions; rotation uploads a version and changes the pinned Terraform
variable.

## Service Identities And IAM

Use dedicated service accounts for API, MCP, worker, dequeuer, migration,
ClamAV updater, Pub/Sub push, and Cloud Scheduler invocation. Do not run
workloads as a default compute service account.

Grant roles at the narrowest resource scope available:

- Runtime identities receive accessor permission on the shared configuration
  secret container. Cloud Run revisions mount only the Terraform-pinned
  version, but Secret Manager IAM cannot restrict access to one version inside
  that secret.
- API and MCP receive only the evidence-bucket object permissions their routes
  require.
- Worker receives evidence-object access and read-only definition-snapshot
  access.
- Dequeuer receives Pub/Sub publisher access but no topic or subscription
  administration.
- Migration receives no GCP data-plane roles beyond its secret.
- ClamAV updater writes definitions; workers cannot write them.
- Pub/Sub push identity receives `roles/run.invoker` only on the worker.
- The Pub/Sub service agent can mint the push identity's OIDC token and operate
  the configured dead-letter policy.
- Scheduler can invoke only the ClamAV update job.

## Database And Migrations

Runtime processes use Supavisor transaction mode. The connection string must
use port 6543 and verified TLS. The database adapter must issue unnamed queries
or otherwise disable named prepared statements, because transaction mode does
not support them.

The adapter satisfies the first clause literally. Every parameterized statement
uses `query_typed`/`execute_typed`, which parse into the **unnamed** statement
and send `Parse`, `Bind`, `Describe`, and `Execute` under a single `Sync` — one
round trip, and nothing that depends on the pooler keeping the same backend
across two. The ordinary `query`/`execute` methods are unusable here because
`tokio_postgres` names every statement they create, and the pooler reassigns the
connection between the `Parse` and the `Bind`.

Those methods require the caller to state each parameter's Postgres type.
`persistence::param` recovers it from the Rust type, so call sites bind values as
before and a type with no mapping is a compile error rather than a runtime one.
This works because the schema is unambiguous: one string type, one JSON type,
one timestamp type, and no enums or domains.

This is verified locally rather than only in production: the compose stack runs
PgBouncer in transaction mode on 6432 with `max_prepared_statements = 0`, so it
refuses named prepared statements exactly as Supavisor does, and the whole
integration-v2 suite runs through it. See `docker/pgbouncer/pgbouncer.ini`.

Initial local pool limits are:

| Runtime | Pool size |
| --- | ---: |
| API | 10 |
| MCP | 10 |
| Worker | 6 |
| Dequeuer | 2 |

At the initial Cloud Run limits this permits at most 68 steady-state client
connections. A conservative revision-overlap estimate remains below 140,
leaving room under the smallest current Supavisor client limit of 200.

The migration job has one task, one connection, no automatic retries, and a
finite timeout. It uses a distinct database role and Supabase's direct endpoint
with certificate and hostname verification. Default Cloud Run internet egress
supports the endpoint's public IPv6 path without VPC egress.

Every runtime performs one read-only schema-history check through its configured
bounded pool before it opens a listener, creates a subscription, or begins
polling. The ordered version, name, and checksum entries in
`refinery_schema_history` must exactly match the migrations embedded in the
binary. An exact incomplete prefix, including a database with no history table,
is rejected as behind and directs the operator to the `migrate` command. Any
extra or divergent entry is rejected as unknown and requires a binary built
with that exact migration history. The check never creates or changes the
history table.

The `migrate` command resolves that credential from the first source that is
set, and fails naming that source when it is set but unusable rather than
falling through to a lower one:

| Order | Source | Used by |
| --- | --- | --- |
| 1 | `PROOFPLANE_MIGRATION_DATABASE_URL_FILE`, a path to a file holding the URL | production, the only variable the job sets |
| 2 | `PROOFPLANE_MIGRATION_DATABASE_URL`, the URL itself | `make migrate` and one-off runs |
| 3 | `PROOFPLANE_CONFIG`, whose `database.url` is used | a run that must match an application configuration |

No application configuration is mounted for the job, so nothing else about the
command may depend on it. The command sets a five-second `lock_timeout` on its
connection before running: refinery takes no advisory lock of its own, so an
unbounded run that meets a conflicting session would queue behind it and hold
the apply open until the job's 900-second timeout. The expand-then-contract
rules migrations follow are recorded in
[`migrations/README.md`](../../../migrations/README.md).

Set the beta `run_execution_token` field from a short, valid suffix derived from the immutable
release digest. Every serving workload and initial delivery resource depends on
the migration job. `terraform apply` therefore waits for successful migration
completion before updating them. A migration failure fails the apply.

Migrations are expand-only in the release that introduces application usage.
Destructive contract steps occur only after old revisions have drained, in a
later release. Exceptions require an announced maintenance window.

## Storage

### Evidence Bucket

Use regional Standard storage in `us-central1`, uniform bucket-level access,
public-access prevention, and Terraform deletion protection. Enable 30-day
soft delete. Do not enable object versioning or a retention lock. Retention
locks remain deferred until Proofplane has an explicit legal retention and
customer-deletion policy.

### ClamAV Definitions Bucket

Keep timestamped, immutable snapshots and a separately published last-good
pointer. Retain live snapshots for seven days, then retain lifecycle deletions
through seven days of soft delete. Disable versioning. Protect the bucket from
Terraform deletion.

### Terraform State Bucket

State protection is described separately because state requires versioning and
restricted operator access, not application runtime access. Unlike the two
buckets above, it is operator-created and not a Terraform resource — see
[Terraform State](#terraform-state).

## Cloud Run Workloads

| Workload | Type | Scaling | Concurrency | CPU | Memory |
| --- | --- | --- | ---: | ---: | ---: |
| API | Service | 0..3 | 20 | 1 | 512 MiB |
| MCP | Service | 0..3 | 20 | 1 | 512 MiB |
| Worker app | Service container | 0..1 | 4 | 1 | 1 GiB |
| Worker clamd | Service sidecar | follows worker | n/a | 2 | 4 GiB |
| Dequeuer | Worker pool | exactly 1 | n/a | 1 | 512 MiB |
| Migration | Job | one task | n/a | 1 | 512 MiB |
| ClamAV update | Job | one task | n/a | 1 | 1 GiB |

API, MCP, and worker use request-based billing. API and MCP allow only
`internal-and-cloud-load-balancing` ingress and have their default URLs
disabled. Their Cloud Run IAM invocation check is disabled because application
OAuth authenticates public traffic and the load balancer is the only ingress
path.

The worker uses `internal` ingress, keeps its default URL for same-project
Pub/Sub delivery, and requires Cloud Run IAM authentication. Only the Pub/Sub
push identity can invoke it. Set request timeout to 600 seconds. The application
container depends on a successful clamd startup probe.

The dequeuer worker pool has no URL, is manually scaled to one instance, and
uses instance-based worker-pool billing. It must shut down gracefully so an
outbox claim is not silently abandoned during revision replacement.

## Pub/Sub

Terraform is the exclusive owner of production messaging resources. Provision
every application topic required by the message catalog plus:

- the worker push subscription;
- a dead-letter topic;
- a persistent pull subscription on the dead-letter topic;
- OIDC push configuration and service-agent IAM.

The worker subscription uses:

- the private worker's default `run.app` message endpoint;
- a dedicated OIDC service account and matching audience;
- 600-second acknowledgment deadline;
- 10-to-600-second exponential retry backoff;
- five maximum delivery attempts;
- seven-day message retention;
- no expiration policy.

The dead-letter inspection subscription retains messages for 31 days and never
expires. Alert when any dead-letter message is observed. Pub/Sub delivery is
at-least-once; handlers must remain idempotent even within the acknowledgment
deadline.

## ClamAV Definitions

Run a scheduled update job every four hours. It is the only workload permitted
to contact the ClamAV CDN. It starts from the last-good snapshot, runs
`freshclam` incrementally, validates the resulting database with pinned ClamAV
tools, uploads an immutable snapshot, and atomically advances the last-good
pointer. A failed update never replaces the pointer.

Worker instances copy the last-good snapshot from GCS into local ephemeral
storage before starting `clamd`; `freshclam` is disabled in worker sidecars.
Workers fail readiness if the last-good snapshot is more than 24 hours old.
Alert after two consecutive updater failures and as the snapshot approaches
the freshness limit.

Configure `clamd` with four worker threads. The Cloud Run service accepts four
concurrent pushes, allowing one daemon to scan multiple documents without
duplicating its definition memory.

## Public Edge, TLS, And DNS

Use one global external Application Load Balancer, one global IPv4 address, and
host-based routing to distinct serverless NEGs. Configure HTTP to redirect to
HTTPS. Use Certificate Manager DNS authorization and a Google-managed
certificate covering `api.proofplane.app` and `mcp.proofplane.app`.

Cloud DNS becomes authoritative for the full `proofplane.app` apex. Before
delegation, export and reproduce every Route 53 record, reduce TTLs, and verify
the Cloud DNS zone independently. At the registrar, replace the Route 53
nameservers with the four Cloud DNS nameservers; do not add both sets as apex NS
records inside Route 53. Keep the old hosted zone during propagation and remove
it only after public resolvers consistently return Google nameservers and all
records resolve correctly.

Terraform manages the Cloud DNS zone and records after importing any zone that
was created manually for testing. Leave apex and `www` application routing
undefined until a website decision is made. DNSSEC is a post-migration
hardening step so a DS mismatch cannot break the initial cutover.

Cloud Armor is not part of the initial deployment.

## Monitoring And Recovery

Use Cloud Logging and built-in Cloud Monitoring metrics. Do not continuously
probe public health endpoints because that would defeat scale-to-zero behavior.
The local deployment command performs smoke checks after apply.

Create passive alerts for:

- Cloud Run 5xx responses and startup failures;
- migration and ClamAV update job failures;
- worker/dequeuer CPU or memory above 80% for a sustained window;
- Pub/Sub oldest-unacknowledged age and any dead-letter message;
- dequeuer worker-pool instance count below one;
- ClamAV snapshot age and repeated update failure;
- budget thresholds.

Custom metric scraping is deferred even though runtimes expose `/metrics`.
Keep default 30-day application-log retention and rely on platform audit logs
for infrastructure mutation history.

Supabase daily backups are accepted for the first deployment; PITR is not a
go-live requirement. The runbook must state a possible database recovery-point
loss of roughly 24 hours and verify that daily backups are active. Evidence
objects have independent GCS soft-delete recovery.

## Cost Guardrails

Create a $100 monthly GCP budget with alerts at 50%, 80%, and 100%. Budgets do
not cap or automatically stop resources.

At current list prices, the low-traffic baseline is approximately $50-55 per
month, excluding Supabase and domain registration:

- dequeuer worker pool: approximately $31.17 before free-tier credit;
- global load balancer: approximately $18.25;
- Cloud DNS: approximately $0.20 plus query volume;
- storage, images, and low-volume managed services: typically under a few
  dollars.

Variable drivers are worker active time, public traffic and egress, evidence
storage and soft-deleted bytes, log ingestion beyond the free allotment, and
new image vulnerability scans.

## Local Release Workflow

`infra/gcp/production/terraform-root.mk` wraps the two routine Terraform
commands, and every phase root includes it, setting only its own state prefix
and default plan name. `make init` initializes the backend, and `make plan`
writes a saved plan, each re-running only when its local inputs change. There is
no `apply` target: applies stay an explicit operator action against a reviewed
plan.

The operator workflow is:

1. Verify local tooling, GCP identity/project, Terraform backend access,
   Supabase backup status, pinned configuration versions, and clean inputs.
2. Apply `01-artifacts` so the regional repository exists.
3. Build the Linux release image and run local image smoke checks.
4. Push the release and any newly pinned ClamAV image to Artifact Registry.
5. Apply `02-foundation`, then create any secret payload version outside
   Terraform.
6. Resolve immutable digests and review the `03-release` plan.
7. Run one `terraform apply` in `03-release`. The migration execution completes
   before serving workloads update.
8. Verify job execution, revision health, Pub/Sub push authentication, public
   TLS endpoints, version output, and a non-destructive end-to-end message.
9. On application failure after migration, roll forward with a corrected binary
   that embeds the applied schema history. An older image cannot restart once a
   newer migration is present, even when that migration is additive. Do not
   reverse an expand migration automatically.

Terraform does not use `local-exec` for builds, migrations, smoke checks, or
DNS registrar changes.

## Deferred Hardening

- Static egress through VPC/NAT and Supabase IP allowlisting.
- Cloud Armor WAF and rate limiting.
- Supabase PITR and scheduled restore exercises.
- DNSSEC after stable Cloud DNS delegation.
- Per-runtime least-content configuration secrets.
- Enforced container vulnerability thresholds.
- Central Prometheus/OpenTelemetry collection and continuous uptime checks.
- CI/CD and automated staged traffic rollout.

## Revisions

- 2026-08-05: Initial spec from the first-production-deployment grilling
  session. Terraform owns Pub/Sub, migrations gate serving revisions in one
  apply, runtime traffic uses transaction pooling, ClamAV definitions are
  centrally mirrored, and Cloud DNS is authoritative for `proofplane.app`.
- 2026-08-05 (ticket 003): The separate bootstrap Terraform root was removed.
  The state bucket is created manually in `us-east1` instead, which drops the
  temporary-local-state root and the migrate-state-into-itself step; the
  production root adopts the bucket through partial backend configuration at
  `terraform init`. Operator access to the bucket is no longer expressed as
  Terraform IAM.
- 2026-08-06 (ticket 010): `infra/gcp/production/Makefile` added as the
  dependency-tracked entry point for `terraform init` and `terraform plan`.
  Apply is deliberately not wrapped.
- 2026-08-14 (#157): Serving runtimes now perform an exact, read-only migration
  history check and never apply migrations. Because older images reject newer
  histories on restart, post-migration recovery rolls forward with a
  schema-matching binary.
- 2026-08-14 (#117): The release image now exists, and this spec previously
  described none of its properties. Base images, platform, runtime user,
  packaged commands, and the exclusion of `seed` are now recorded above. The
  build runs emulated for `linux/amd64` rather than cross-compiled, because
  `openssl-sys` links dynamically and `aws-lc-sys` builds through `cmake`. Those
  two together make a multiarch cross toolchain the more fragile path. Cargo
  cache mounts confine the cost to the first build. This section also said to
  mirror "a pinned official ClamAV image". `run.tf` in fact consumes two ClamAV
  digests, and #117 mirrors only the pinned base that both derived images in
  #121 start from.
- 2026-08-14 (#117): Image retention remains partly unmet. The cleanup policies
  in `artifacts.tf` delete an untagged version after 30 days and keep the 20
  most recent versions. No policy deletes a tagged version, so tagged releases
  accumulate rather than stay bounded. `artifacts.tf` belongs to #118, which
  owns the correction. Retention matters more after #157: a runtime accepts work
  only when its own embedded history matches the database, so an image and the
  schema it was built against are now a pair.
- 2026-08-15: The single production root became three ordered roots:
  `01-artifacts`, `02-foundation`, and `03-release`. The registry had to leave
  the root that consumes image digests, because an image cannot be pushed to a
  repository that the same apply creates. Splitting the release out as well
  removed `release_enabled`, the boolean that switched roughly 30 resources
  between `count = 0` and `count = 1` so that one root could hold two
  half-configurations. Each root is now a complete configuration for its stage.
  `03-release` reads `02-foundation` through `terraform_remote_state`.
  `01-artifacts` publishes no output the others read, but it enables
  `cloudresourcemanager` and `serviceusage`, which every later root needs, so
  the order still binds. `infra/gcp/production/Makefile` became
  `terraform-root.mk`, which each root includes after setting its own state
  prefix and plan name. Nothing had been applied, so no state was moved.
