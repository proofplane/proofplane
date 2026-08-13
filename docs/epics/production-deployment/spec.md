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
| Startup behavior | API, MCP, worker, and dequeuer never run migrations during startup. |
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

State goes in a bucket the operator creates manually, before the production
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

The production root declares a partial `gcs` backend. The bucket name and the
`proofplane/production` prefix are supplied at `terraform init`:

```sh
make init TF_STATE_BUCKET=YOUR_STATE_BUCKET   # in infra/gcp/production
```

Terraform caches the backend configuration in `.terraform/terraform.tfstate`,
so later inits need no arguments. Because the bucket is not a Terraform
resource, its protection is whatever the operator configured at creation;
`prevent_destroy` does not apply to it.

Secrets and secret-version payloads never appear in Terraform configuration or
state.

## Artifact Registry And Images

Create one regional Docker repository. A local build produces a Linux image,
pushes a content-addressed artifact, resolves its `sha256` digest, and supplies
that digest to Terraform. Never deploy mutable tags.

The Proofplane image contains all production commands. Each Cloud Run resource
overrides the command instead of building a process-specific image. Mirror a
pinned official ClamAV image into the same regional repository so production
does not depend on Docker Hub availability or mutable tags.

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

`infra/gcp/production/Makefile` wraps the two routine Terraform commands.
`make init` initializes the backend, and `make plan` writes a saved plan,
each re-running only when its local inputs change. There is no `apply` target:
applies stay an explicit operator action against a reviewed plan.

The operator workflow is:

1. Verify local tooling, GCP identity/project, Terraform backend access,
   Supabase backup status, pinned configuration versions, and clean inputs.
2. Build the Linux release image and run local image smoke checks.
3. Push the release and any newly pinned ClamAV image to Artifact Registry.
4. Resolve immutable digests and review `terraform plan`.
5. Run one `terraform apply`. The migration execution completes before serving
   workloads update.
6. Verify job execution, revision health, Pub/Sub push authentication, public
   TLS endpoints, version output, and a non-destructive end-to-end message.
7. On application failure, reapply the previous application digest. Do not
   attempt to reverse an expand migration automatically.

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
