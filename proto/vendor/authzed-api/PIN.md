# Authzed API Proto Pin

Proofplane generates its SpiceDB Tonic bindings from this local proto tree.
Normal Rust builds must not fetch proto inputs from the network.

- Authzed API source: `authzed/api` tag `v1.45.4`
- Authzed API source commit: `5253a15fb25cb0d1d6b334039613ce0e824db23b`
- Vendored Authzed definitions: `authzed/api/v1/core.proto`, `debug.proto`,
  `permission_service.proto`, and `schema_service.proto`

The imported option and validation protos are vendored beside those definitions
so `protoc` can parse the pinned Authzed input. The dependency pins used by that
Authzed release are recorded in its `buf.lock`:

- `buf.build/bufbuild/protovalidate` commit `0409229c37804d6187ee0806eb4eebce`
- `buf.build/envoyproxy/protoc-gen-validate` commit `daf171c6cdb54629b5f51e345a79e4dd`
- `buf.build/googleapis/googleapis` commit `61b203b9a9164be9a834f58c37be6f62`
- `buf.build/grpc-ecosystem/grpc-gateway` commit `4c5ba75caaf84e928b7137ae5c18c26a`

Upgrade this directory intentionally from a new Authzed API revision and keep
`build.rs` pointed at local files only.
