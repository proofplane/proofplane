# Production Runtime Adapters Spec

## Goal

Run the existing evidence pipeline against production Google Cloud services.
Local filesystem and Pub/Sub emulator behavior remain first-class development
paths, but runtime composition must not encode them as the only supported modes.

## Object Store Runtime

Keep `ObjectStore` as the adapter contract and add concrete implementations for
filesystem and GCS. Runtime dependencies use a concrete enum:

```rust
pub enum RuntimeObjectStore {
    Filesystem(FilesystemObjectStore),
    Gcs(GcsObjectStore),
}
```

The enum delegates `put`, `get`, `head`, `copy`, and `delete` without dynamic
dispatch. Services and handlers depend on the enum instead of the current
filesystem concrete type.

GCS configuration defines the bucket and object-key prefix. GCS always uses
Google application default credentials; the existing endpoint-override and
anonymous-credential fields are removed because Proofplane does not run a GCS
emulator. The configured prefix is prepended outside the logical
workspace-scoped `ObjectKey` and is never persisted in document rows.

Uploads and reads stream. Copy may use GCS server-side copy/rewrite, but must
verify destination metadata before finalization marks a document uploaded. GCS
uploads record the calculated SHA-256 digest as `proofplane-sha256` custom
metadata. Finalization compares the copied key, content type, length, and digest
with the persisted document metadata and removes a mismatched destination before
returning the work for retry.

## Pub/Sub Runtime

`GoogleCloudPublisher` already uses the Google client library, but the dequeuer
currently refuses startup unless `PUBSUB_EMULATOR_HOST` is present. Remove that
guard. Client construction follows the SDK convention:

- emulator variable present: connect anonymously to the emulator;
- emulator variable absent: use application default credentials and the
  configured project.

The application never provisions or reconciles topics, subscriptions, retry
policies, dead-letter resources, or delivery IAM. Terraform is the exclusive
owner of those resources in production, and local test setup remains the owner
when the emulator is used. The dequeuer receives publisher-only permission for
the configured application topics.

Push endpoint authentication is deployment-owned for the MVP. The worker must
document how Cloud Run/IAM or an equivalent ingress policy restricts the
endpoint; application-level verification of Google-signed push tokens is a
follow-up unless deployment cannot enforce it.

## Configuration And Errors

Unsupported-backend errors disappear once GCS is implemented. Storage errors
distinguish not found, authentication/permission, unavailable/timeout, integrity
failure, and invalid keys well enough for stable API/worker decisions.

No credentials are accepted inline in YAML. Production uses workload identity
or application default credentials.

## Verification

Local integration-v2 tests use `FilesystemObjectStore` and require no cloud
credentials. The shared object-store contract runs against filesystem storage
unconditionally. A real-GCS variant is ignored by default and can be run by an
operator who supplies application default credentials and a disposable bucket:

```bash
PROOFPLANE_GCS_TEST_BUCKET=<disposable-bucket> cargo test object_storage::tests::gcs_satisfies_the_shared_object_store_contract -- --ignored --exact
```

The GCS contract creates unique per-run prefixes and deletes every known object
after success, assertion failure, or task panic. Provisioning the bucket and its
test identity is environment-owned; this epic adds no CI workflow or identity.
The contract covers upload, head, streamed read, server-side copy, idempotent
delete, prefix isolation, logical keys, checksums, invalid keys, and
cross-workspace rejection. Unit tests additionally cover configuration,
deterministic error mapping, metadata mapping, and streaming failures.

Pub/Sub emulator integration-v2 tests remain because Pub/Sub local behavior is
a separate decision. A credential-free construction test proves the dequeuer no
longer requires the emulator variable; normal CI integration-v2 coverage
verifies the configured Pub/Sub mode without a separate deployment smoke test.

## Revisions

- 2026-08-19: Made real-GCS verification an explicit, configuration-driven
  contract command. Removed CI workflow and test-identity provisioning from the
  epic while retaining filesystem-backed integration-v2 coverage.
- 2026-08-05: Moved production topic, subscription, delivery, and IAM ownership
  from dequeuer startup to Terraform. The runtime now publishes only; local
  emulator test setup may still provision disposable resources.
- 2026-06-11: Added the production Pub/Sub gap discovered while reconciling
  the original runtime plan with current code.
- 2026-06-11: Removed the GCS emulator path. Local tests use filesystem storage;
  CI integration-v2 tests exercise the real GCS adapter and clean up isolated
  prefixes.
