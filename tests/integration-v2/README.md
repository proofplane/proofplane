# integration-v2

This black-box suite exercises Proofplane the way a real client does — over HTTP
and MCP — and asserts only on what a real client can observe.

Run with `cargo test --test integration-v2`. Docker must be available.

The suite owns one application topology for the lifetime of the test binary. A
dedicated OS thread runs a long-lived multi-thread Tokio runtime containing one
Postgres container and pool, one transactionally seeded SOC 2 reference catalog, one
filesystem object store, a fake clamd server,
one worker, one observable push proxy, one deltio Pub/Sub project, one dequeuer,
one controlled auditor identity-provider boundary, and one API and MCP server.
`harness::app().await` returns a lightweight
cloneable handle to those shared servers; it does not bind application resources
to the calling test's Tokio runtime.

Both exposed `TestServer`s use real HTTP transport. Tests still construct every
request explicitly with `app.app_server().get(...)`, `.post(...)`, and the other
`axum_test` request builders. `McpClient` remains separate MCP-specific support
and connects to `app.mcp_server()` over streamable HTTP.

## The rule that drives everything

**A test may only do what a real client could do.**

Arrange through the product's own entry points. Observe through responses,
rendered pages, MCP tool results, and audit logs. Never reach behind them.

That means:

- No database handle. There is no `postgres()` on `TestApp` and no raw SQL.
- No in-process services. Don't construct `DocumentUploadGrantService` or any
  other service to set something up or to check a result.
- No injected stubs for things the real flow produces. Agent connections come
  from walking the OAuth flow, not from inserting an `agent_connections` row.

The fixed reference catalog is suite topology, not test arrangement. After migrations,
the harness inserts exactly the v1 SOC 2, CC6.1, and CC7.1 definitions before the servers
start. Test bodies never invoke the seeder or see its raw IDs; every completed `Scenario`
exposes the same projections through `scenario.framework("soc2")` and chained requirement
lookup. This does not relax the prohibition on per-test database setup.

When behavior genuinely isn't reachable this way, cover it at the appropriate
lower-level test boundary and say so. Reaching for the database to close a gap
defeats the point of this suite. For example, forcing a 500 out of token
verification needs a stub verifier and does not belong in integration-v2.

## Arrange in the test body

`ScenarioBuilder` sets up users, workspaces, and prerequisite evidence, controls,
policies, clean evidence submissions, clean policy documents, evidence-control mappings,
and policy-control mappings. Write it inline so the reader sees the whole
arrangement without
following a call:

```rust
#[tokio::test]
async fn grant_url_redeems_once_and_opens_a_scoped_session() {
    let app = harness::app().await;
    let subject = "auth0|upload-redeem";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Redeem")
        .with_evidence("Redeem", "Quarterly access review")
        .build()
        .await;

    let evidence_id = scenario
        .workspace("Redeem")
        .evidence("Quarterly access review")
        .id;

    let token = authorize_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    // ...
}
```

Do **not** collapse that into `let client = connect(&app, "auth0|upload-redeem", "Redeem").await;`.
The helper saves four lines and hides which permissions the connection holds,
which workspace it belongs to, and that an OAuth flow ran at all — exactly the
things the rest of the test depends on.

Evidence, controls, policies, evidence-control mappings, and policy-control mappings are
deliberate fixture
exceptions. When they are only prerequisites, declare records with
`.with_evidence(workspace_name, title)`, `.with_control(workspace_name, code, title)`, or
`.with_policy(workspace_name, name)`, then declare an evidence-control relationship with
`.with_evidence_control_mapping(workspace_name, evidence_title, control_code, rationale)`
or a policy-control relationship with
`.with_policy_control_mapping(workspace_name, policy_name, control_code)`.
Declare a suite-catalog requirement relationship with
`.with_control_framework_requirement(workspace_name, control_code, framework_code,
requirement_code)` after its control. The builder resolves both codes through the immutable
suite-seeded catalog and includes the resulting requirement ID in the control's MCP creation.
Policy declarations require their workspace to appear first. Evidence-control mapping
declarations require their evidence and control declarations first; policy-control
mapping declarations require their policy and control declarations first.
The builder still walks the real OAuth flow, authorizes separate workspace-specific
fixture connections with only `WriteEvidence` or `WriteControls`, connects through MCP,
and calls the corresponding create or mapping tool. Fixture policy records are
deliberately minimal and have no description; prerequisite control mappings are separate
declarations. Retrieve records through
workspace-scoped lookup:
`scenario.workspace(workspace_name).evidence(title)` or
`scenario.workspace(workspace_name).control(code)`, or
`scenario.workspace(workspace_name).policy(name)`. Later record types should follow this
convention when the builder supports them.

Clean terminal documents are also fixture exceptions when the upload itself is only
arrangement. Declare the parent first, then use
`.with_evidence_document(workspace_name, evidence_title, filename, bytes, valid_from,
valid_until)` or `.with_policy_document(workspace_name, policy_name, filename, bytes)`.
Evidence filenames must be unique within one evidence item, and a policy may declare only
one document. The builder copies the bytes, issues and redeems a real MCP grant, uploads
the multipart file through the browser route, waits for scan and finalization delivery,
and rereads the MCP projection before requiring `upload_status == "uploaded"`. Evidence
documents use a dedicated workspace connection scoped to read/write evidence submissions;
policy documents reuse the policy fixture connection, adding read access so `get_policy`
can verify the result. Evidence fixtures run sequentially in declaration order.

Retrieve the typed results with
`scenario.workspace(name).evidence(title).submission(filename)` and
`scenario.workspace(name).policy(name).document()`. Pending, finalizing, malicious,
failed, replacement, and archived states are not builder fixtures; arrange those states or
transitions explicitly in the story that observes them.

When an MCP operation is the behavior under test, keep its authorization, complete
arguments, and tool call directly in the test. Evidence-creation and control-creation
tests therefore call `create_evidence` and `create_control` explicitly, policy-creation
and name-reuse tests call `create_policy` explicitly, and mapping tests call their map,
unmap, attach, or detach tool explicitly. Batch operations under test always remain in
the test body. Their subject operations never move into `ScenarioBuilder`.
Connections used for the test's actual operations also remain explicit whenever their
permissions or audit attribution matter.

Every completed scenario also carries the suite's immutable framework metadata. Resolve
it without involving the builder or database:

```rust
let soc2 = scenario.framework("soc2");
let cc61_id = soc2.requirement("CC6.1").id;
```

`TestFramework` and `TestFrameworkRequirement` expose their IDs and complete projection
fields. Tests must not hard-code the fixture UUIDs.

## No negative assertions

**Never assert that something is absent. Assert what is present instead.**

```rust
// No. Passes if the field is renamed, encoded, or leaks only its value.
assert!(!body.contains("object_key"));

// Yes. Pin what the response is, and nothing can hide inside it.
assert_eq!(fields, ["authorized_at", "client_name", "id", "last_used_at", "status"]);
```

The problem is that you cannot enumerate everything a response should *not*
contain, so a passing negative assertion tells you nothing. `!body.contains("object_key")`
still passes when the object key's *value* is rendered without its field name —
the leak you actually care about. And it goes stale silently: rename the field and
the assertion keeps passing forever.

Pin the positive shape and the negatives come free. If you assert a JSON object's
complete key set, no unwanted key can appear. If you assert a page equals the
expected generic recovery text, nothing can leak into it.

There are exactly two sanctioned exceptions, because both are complete
observations rather than guesses about absence:

1. **An empty complete collection.** `assert!(logs.is_empty())` after
   `capture_audit_logs` is an equality over everything the sink captured for that
   request, and `list_evidence_submissions` returning `[]` is an equality over the
   full set. The line is whether the observation is enumerable: a list endpoint's
   response is, a rendered HTML page is not.
2. **`assert_ne!` on distinctness.** "Each file becomes its own submission" is
   inherently a claim that two ids differ, and both are fully observable. The
   positive rephrasing (collect into a set, assert its length) says the same thing.

Anything else that reads `assert!(!…)` is a bug in the test. Concealment is
positive too — `assert_status_not_found()` asserts the status *is* 404, which is
the pattern to reach for when proving something is hidden.

Pinning the positive shape also tells you when your mental model is wrong. The
assertion on the upload page's file rows caught that the status column renders
`Uploading`, not the raw `pending`, and that rows come back newest-first. A
`!contains("object_key")` would have passed while knowing neither.

## Helpers

Extract sparingly, and only when both are true:

1. The thing is genuinely repeated — several call sites, not two.
2. Extracting it doesn't hide what a test is about.

Setup is usually the second kind. Formatting, parsing, and multi-step mechanical
sequences are usually the first.

Keep helpers **private and local to the file that needs them**. A helper used by
one test file does not belong in `support/`. If a second file later needs it,
move it then.

Some real examples that earned their place as shared evidence-document machinery:

- `auditor_access::invite_token(&Url)` — validates and extracts the one bearer-secret query
  parameter shared by auditor browser stories.
- `auditor_access::assert_portal_read_audit(...)` — pins the complete safe audit shape reused
  by JSON and HTML portal catalog reads.
- `http::local_path(&str)` — grant URLs are absolute against the public API base,
  which is not where the test server listens.
- `http::request_cookie(&str)` — turns a `Set-Cookie` header into a `Cookie` header.
- `documents::upload_form(bytes, filename)` — an owner-neutral multipart body used by
  evidence and policy upload tests.

File-specific helpers remain local when their behavior or assertions differ.

Prerequisite product state supported by `ScenarioBuilder` belongs there, not in a
file-local wrapper around one create or mapping call. A
`connect(app, subject, workspace)` wrapper around the whole arrangement still hides too
much.

## Module size

Treat 500 lines as a review signal for integration-v2 test modules, not as a
hard limit. When a file approaches that size or mixes several independent test
stories with substantial local machinery, split it into a directory module
with small story-focused files and a local `helpers.rs`. Keep `mod.rs` limited
to module declarations and genuinely shared imports or constants.

Splitting must preserve the rest of this guide: authorization, permissions,
declarations, and operations under test stay explicit in each story, while
only repeated protocol mechanics and complete assertion helpers are shared.

## `support/` is shared machinery, not conveniences

| Module | What it is |
| --- | --- |
| `agent_connections` | Public-listing lookup of an authorized connection ID by subject and client name, used when audit attribution is the assertion rather than connection listing itself. |
| `auditor_access` | Exact invite-token parsing, a hosted-login prerequisite that accepts explicit workspace, invitation, subject, and email values, and complete secret-free auditor portal read-audit assertions shared across session, JSON, download, and browser stories. |
| `harness` | The suite-owned runtime and shared application topology. `TestApp` exposes the two client-facing HTTP test servers, code-scoped auditor identity controls and recorded exchanges, pipeline events, proxy and ClamAV controls, `login`, and `capture_audit_logs`. |
| `config` | The hard-coded `AppConfig`. No YAML, no env vars. |
| `documents` | Shared owner-neutral multipart construction for evidence and policy document browser uploads. |
| `evidence_documents` | Coverage timestamps and canonical evidence-document paths. |
| `http` | Mechanical translation of public URLs and response cookies into local test-request values. |
| `json` | Complete JSON object-key collection and RFC 3339 timestamp assertions shared across API and MCP tests. |
| `auth` | `FakeTokenVerifier` — the bearer token *is* the `auth0_sub`. Plus `assert_unauthorized`. |
| `auth0` | A fake upstream tenant on `127.0.0.1:9099` for owner OAuth, plus the injected auditor identity-provider boundary. Auditor outcomes and recorded exchanges are keyed by a test-unique authorization code; unregistered codes are rejected. |
| `clamd` | Concurrent test-only INSTREAM server. Chooses clean, EICAR, or scanner-error replies from uploaded bytes and provides a scoped content-matched hang after reading a complete scan request. |
| `pubsub` | Suite-wide deltio container with container-to-host routing for push delivery. |
| `worker` | Real worker server plus the `0.0.0.0` push proxy, post-response pipeline event stream, request-ID-reserved holds, and one-shot redelivery injection. |
| `oauth` | Walks the real authorize → consent → token flow and returns an access token. |
| `mcp` | `McpClient` — a real `rmcp` client over the streamable HTTP transport. |
| `reference_data` | One support-only transaction that inserts only SOC 2, CC6.1, and CC7.1 before server startup; raw IDs stay private. |
| `scenario` | `ScenarioBuilder` for users, workspaces, and OAuth/MCP/browser/worker-backed prerequisite evidence, controls, control-requirement links resolved through the seeded catalog, minimal policies, clean terminal documents, evidence-control mappings, and policy-control mappings; every result also carries typed chained lookups and the fixed reference-catalog projections. |
| `audit_log` | The tracing sink behind `capture_audit_logs`. |

`TestApp` deliberately has **no request helpers**. It hands you servers; tests
write their own requests. Adding `app.create_evidence(...)` would put the
product's setup path behind a method nobody reads, and the next person would
assume it's how the product works.

## Writing a test

- One `harness::app().await` per test. Each call clones a handle to the same
  suite-owned servers.
- Name after the observable outcome: `revoked_connection_token_is_unauthorized`,
  not `test_revocation`.
- Prefer a few tests that each tell one story over one test that asserts
  everything. Split reauthentication, token-state, and public-route behavior
  when they are independently observable.
- Assert on the shape clients depend on, positively and completely — see the
  key-set assertion in `agent_connections.rs`.
- Comment the *why* when an assertion looks arbitrary. `// The grant is a
  one-shot bearer secret: replaying it reveals nothing.` earns its line.

## Things that will bite you

- **Postgres is shared across the whole binary.** Every test needs unique
  `auth0_sub` values and workspace names. Prefix subjects with the file or test:
  `auth0|upload-redeem`.
- **A user gets one workspace.** A second `POST /workspace` is a conflict, so
  cross-tenant tests need a second user.
- **Reusing a `client_name` for the same user** resolves to the existing agent
  connection and never reaches consent. Give each connection its own name.
- **An empty OAuth scope is rejected**, so there is no such thing as a
  zero-permission connection. Use one narrow permission instead.
- **Timestamps come back with millisecond precision** (`2026-01-01T00:00:00.000Z`).
  Write constants in that form so the same value can be sent and asserted.
- **HTTP strips trailing whitespace from header values**, so a padded
  `Bearer <token> ` arrives clean. That rejection is only testable as a unit test.
- **Both servers run on `http_transport()`** so application work stays on the
  suite runtime while requests can originate on any test runtime. Tests still
  build REST requests directly through the API `TestServer`; `McpClient` dials
  the MCP server's real address.

## Translating behavior into client-visible tests

Build each integration-v2 test around the public path to the behavior rather
than internal state or service calls.

1. Find the client-visible path to the behavior. Most prerequisite setup has an
   MCP tool (`create_evidence`, `create_policy`,
   `manage_evidence_submissions`, `create_auditor_access_link`).
2. Replace each database assertion with the read that exposes the same fact —
   `list_evidence_submissions` instead of `SELECT ... FROM evidence_submissions`,
   a second request returning 404 instead of `redeemed_at IS NOT NULL`.
3. Split the mega-tests into focused ones.
4. Cover behavior that has no client-visible path at the appropriate lower-level
   boundary and note why it is outside integration-v2 in the PR.

Client-visible tests are strongest when they mint real tokens through the real
flow instead of faking an agent connection with an inserted row and a stub
verifier.
