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

GCS configuration already defines bucket, optional endpoint override,
credentials mode, and object-key prefix. `application_default` uses Google
application default credentials; `anonymous` exists only for a local emulator.
The configured prefix is prepended outside the logical workspace-scoped
`ObjectKey` and is never persisted in attachment rows.

Uploads and reads stream. Copy may use GCS server-side copy/rewrite, but must
verify destination metadata before finalization marks an attachment uploaded.

## Pub/Sub Runtime

`GoogleCloudPublisher` already uses the Google client library, but the dequeuer
currently refuses startup unless `PUBSUB_EMULATOR_HOST` is present. Remove that
guard. Client construction follows the SDK convention:

- emulator variable present: connect anonymously to the emulator;
- emulator variable absent: use application default credentials and the
  configured project.

Topic and push-subscription provisioning remains idempotent. Production
provisioning errors fail startup with actionable context.

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

GCS adapter contract tests run against an emulator when one is available in
CI. SDK request construction and error mapping may use focused fakes, but the
shared object-store contract must also run against filesystem.

Pub/Sub emulator integration tests remain. A credential-free construction test
must prove the dequeuer no longer requires the emulator variable before a
production deployment smoke test validates real provisioning.

## Revisions

- 2026-06-11: Added the production Pub/Sub gap discovered while reconciling
  legacy stories 011-014 with runtime code.
