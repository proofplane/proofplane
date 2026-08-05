# ADR 0002: Deploy Production on Google Cloud Run

**Status:** Accepted · **Date:** 2026-08-05

## Context

Proofplane has five production process shapes: two public HTTP services, one
authenticated Pub/Sub push worker, one continuously running outbox dequeuer,
and one database migration command. It also needs malware scanning, durable
object storage, public TLS, and deployment from a developer workstation before
CI/CD exists.

The production database already exists in Supabase. The application is being
refactored separately, so deployment infrastructure must make its application
compatibility requirements explicit without temporarily implementing fake GCS
or Pub/Sub behavior.

## Decision

Deploy Proofplane in a dedicated, pre-created GCP project in `us-central1`:

- Run API and MCP as public Cloud Run services behind one global external
  Application Load Balancer. Route `api.proofplane.app` and
  `mcp.proofplane.app` through Google-managed TLS certificates. Restrict the
  services to load-balancer ingress and disable their default `run.app` URLs.
- Run the worker as a private Cloud Run service invoked only by an
  OIDC-authenticated Pub/Sub push subscription. Run `clamd` as a sidecar.
- Run the dequeuer as a single manually scaled Cloud Run worker-pool instance.
- Run migrations through a dedicated Cloud Run Job. A changing
  `run_execution_token` executes and awaits migrations before Terraform updates
  any serving workload.
- Build one immutable Proofplane release image containing all production
  commands. Select the command per Cloud Run resource and deploy by digest.
- Let Terraform exclusively own production Pub/Sub topics, subscriptions,
  dead-letter policy, push authentication, and IAM. Runtime processes publish
  or consume but do not provision messaging infrastructure.
- Store evidence and centralized ClamAV definition snapshots in separate
  private Cloud Storage buckets. A scheduled update job is the only process
  that contacts the ClamAV CDN.
- Use Supabase Supavisor transaction mode for runtime traffic after the database
  adapter stops using prepared statements. Use a separate verified-TLS direct
  connection and credential for migrations.
- Keep Terraform state in a versioned, deletion-protected GCS bucket created by
  a one-time bootstrap root.
- Delegate the `proofplane.app` hosted zone to Cloud DNS at the registrar.
- Run image build/push and Terraform locally. Terraform describes the ordered
  release; post-apply smoke checks remain an explicit operator action.

Production deployment is blocked until the release gates in the production
deployment spec are satisfied. Infrastructure code must not disguise missing
native GCS, Pub/Sub, TLS, migration, database-pool, or stateless-MCP behavior.

## Consequences

- API and MCP can scale to zero, and the worker can scale to zero independently.
- The dequeuer creates an approximately $31/month list-price compute floor; the
  global load balancer adds approximately $18/month.
- The private worker does not need application-level validation of Google push
  tokens because Cloud Run IAM performs authentication and authorization.
- Terraform apply becomes an imperative release boundary for migrations. A
  failed migration stops downstream updates, but a completed expand migration
  is not rolled back when an application smoke check fails.
- Application migrations must be expand/contract compatible with both old and
  new revisions. Contract migrations occur in later releases.
- One complete runtime configuration secret deliberately grants every runtime
  access to the same configuration for now. Separate service identities still
  limit access to GCP resources.
- Dynamic internet egress leaves Supabase protected by verified TLS and
  credentials but not an IP allowlist. Static egress and network restrictions
  are deferred.
- Platform metrics and logs are available at launch; custom Prometheus scraping
  is deferred.

## Alternatives Considered

- **Cloud Run session affinity or a shared MCP session store:** rejected for the
  initial deployment. Proofplane's MCP tools are request/response operations,
  and `rmcp` supports stateless mode directly.
- **A normal Cloud Run service for the dequeuer:** rejected because request-led
  service lifecycle is a poor fit for a continuously polling process.
- **Application-owned Pub/Sub provisioning:** initially considered, then
  rejected to keep infrastructure ownership and privileged Pub/Sub mutation
  permissions out of the long-running dequeuer.
- **Per-runtime configuration secrets:** deferred in favor of one complete YAML
  secret while the configuration model is being refactored.
- **Direct VPC egress and Cloud NAT:** deferred because static egress can add
  cold-start latency or require an always-on connector. Verified TLS remains a
  release gate.
- **Cloud Armor, Supabase PITR, and custom metric scraping:** deferred for the
  first production deployment.
