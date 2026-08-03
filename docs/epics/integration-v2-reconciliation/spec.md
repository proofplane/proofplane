# Integration-v2 Reconciliation Spec

## Goal

Restore `integration-v2` after rebasing it onto the Auth0 auditor-authentication
cutover, then recover client-visible coverage for functionality that landed on
`main` while the suite was being rewritten.

The result is one compiling black-box integration suite that exercises current
production contracts. Do not restore the deleted custom mailer or the removed
`tests/integration/` suite merely to preserve obsolete test arrangements.

## Current State

`cargo test --no-run` fails because integration-v2 still imports the removed
`MailConfig`, `MailAdapterConfig`, and `proofplane::mailer` interfaces. Its
auditor session and portal stories still arrange the retired Proofplane OTP and
mail-delivery flow, while production now delegates auditor authentication to
Auth0 Passwordless Email.

The rebase also deleted the legacy integration tests that covered two features
added to `main` after integration-v2 diverged:

- agent-native evidence uploads through
  `prepare_evidence_submission_upload` and
  `PUT /agent-evidence-uploads/{upload_id}`; and
- agent-native policy document uploads through
  `prepare_policy_document_upload` and
  `PUT /agent-policy-document-uploads/{upload_id}`.

Integration-v2 currently references none of those four public entry points.
The shipped contracts remain defined by the
[Auditor Auth0 Passwordless](../auditor-auth0-passwordless/spec.md),
[Agent-Native Evidence Uploads](../agent-native-evidence-uploads/spec.md), and
[Agent-Native Policy Document Uploads](../agent-native-policy-document-uploads/spec.md)
specs. This epic restores their integration-v2 coverage; it does not redefine
their product behavior.

## Test Boundary

Continue following `tests/integration-v2/README.md`:

- arrange and observe product state through HTTP and MCP;
- keep Postgres, the worker, dequeuer, filesystem object store, fake clamd, and
  deltio Pub/Sub topology real inside the suite harness;
- assert complete client-visible responses, lifecycle projections, and audit
  records instead of querying database rows; and
- keep helpers limited to repeated protocol mechanics rather than hiding the
  authority, permissions, declarations, or operations under test.

Auth0 is a genuine external adapter boundary. Use the auditor identity-provider
fake already supported by `AppDependencies`, or an equivalent fake upstream,
to control successful, rejected, mismatched, and unavailable identity outcomes
without a live Auth0 tenant. Every Proofplane route, authentication transaction,
grant check, session write, cookie, and portal request remains real. Do not add
a production compatibility mailer or a test-only OTP route.

Port observable contracts, not white-box test mechanics. Persistence and state
machine invariants that already have colocated lower-level coverage do not need
duplicate database assertions. A missing externally observable guarantee must
be exercised through the public flow, adding a harness control only at a real
external dependency boundary when deterministic failure injection is required.

## Auth0 Harness And Auditor Coverage

Remove mail configuration and `TestMailAdapter` from integration-v2 support.
Configure `AppDependencies::auditor_identity_provider` and replace OTP-oriented
session helpers with one explicit hosted-login sequence:

1. open a valid invitation;
2. post the login start and capture the Auth0 authorization redirect;
3. obtain the generated state and supply a controlled identity-provider result;
4. call the Auth0 callback; and
5. carry the resulting auditor session cookie into portal and download reads.

Integration-v2 must cover the client-visible Auth0 contract:

- a valid matching, verified identity creates one scoped session, renders the
  portal, and emits complete secret-free lifecycle audit records;
- state replay, mismatched or unverified email, invalid callback input, revoked
  grant, and provider rejection create no usable session;
- provider unavailability maps to the stable retryable browser outcome;
- concurrent callback attempts have one session-creating winner;
- removed JSON and browser OTP endpoints remain unavailable; and
- logout, grant revocation, review-period filtering, portal concealment, safe
  downloads, and browser escaping retain their existing behavior when sessions
  originate from Auth0.

Transaction digest storage, PKCE construction, and claims-policy edge cases
remain covered at their lower-level boundaries unless a client-visible outcome
is missing. Integration-v2 must not reach into Postgres to inspect raw state,
nonce, verifier, or session rows.

## Agent-Native Evidence Upload Coverage

Exercise the complete trusted-runtime workflow: authorize an MCP connection,
prepare one declared evidence upload, perform the returned raw HTTP transfer,
poll the preallocated submission through scan and finalization, and assert the
complete projection and safe success audits.

The integration-v2 matrix must include:

- exact transfer-descriptor shape without file bytes or a local path;
- permission denial and missing or cross-workspace evidence concealment;
- active-evidence eligibility through the public preparation and transfer flow;
- invalid authority, path mismatch, declaration/header mismatch, checksum or
  length mismatch, configured body limit, and interrupted transfer;
- a matching retry after success and concurrent valid transfers converging on
  one submission and document;
- correct agent provenance, one scan/finalization lifecycle, no false success
  audits, and unchanged human browser uploads.

Paused and retired evidence eligibility remains a lower-boundary coverage gap:
the public product exposes evidence status but no status-mutation entry point,
so integration-v2 cannot arrange either state without violating its client-only
rule. Deterministic storage and completion-transaction failures also remain at
their existing lower-level boundaries because the suite has no injectable
filesystem or Postgres dependency control. Closing either gap must use a real
public mutation or external dependency boundary; it must not add database
access, filesystem controls, or production/test-only routes to integration-v2.

Assertions use `get_evidence_submission`, HTTP responses, audit capture, and
pipeline events. They must not inspect grant, submission, document, or outbox
tables directly.

## Agent-Native Policy Document Upload Coverage

Exercise the policy-specific MCP preparation and raw transfer contract through
`get_policy` polling and the real scan/finalization pipeline. Reuse only
transport-level helpers earned by the evidence tests; keep policy authority and
domain assertions explicit.

The integration-v2 matrix must include:

- successful preparation, transfer, polling, finalization, provenance, and
  secret-free audits;
- permission denial and missing, archived, cross-workspace, or already
  documented policy rejection;
- invalid authority and declaration, interrupted or mismatched transfer,
  configured body limit, and matching replay;
- concurrent attempts under one grant converging on one document;
- different machine grants and machine-versus-browser uploads selecting one
  current document without implicit archive or replacement;
- stable storage or transaction failure outcomes where deterministic boundary
  controls exist; and
- unchanged human management, download, archive, replacement, and concealment
  behavior.

Assertions use `get_policy`, public HTTP responses, audit capture, and pipeline
events. They must not inspect grant, policy-document, or outbox tables directly.

## Completion Contract

The epic is complete when:

- `cargo test --no-run` compiles all targets without mailer compatibility code;
- `cargo test --test integration-v2` passes against the current suite topology;
- focused Auth0, agent-evidence-upload, and agent-policy-upload tests pass alone
  and as part of the full target;
- the documented integration-v2 client-boundary rules remain true; and
- `make check` passes without restoring `tests/integration/`.

## Revisions

- 2026-08-03: Reconciled agent evidence upload coverage with integration-v2's
  strict black-box boundary. Active evidence is covered end to end; paused and
  retired arrangement plus deterministic storage and transaction failures stay
  at lower test boundaries until a public mutation or injectable external
  dependency boundary exists.
- 2026-08-03: Initial reconciliation after rebasing integration-v2 onto the
  Auth0 auditor cutover and agent-native evidence and policy upload work.
