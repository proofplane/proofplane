# Proofplane

Proofplane is compliance evidence infrastructure for small startups. It keeps
controls, policies, and evidence in one workspace, and it lets a trusted AI agent
do the data-plane work through the Model Context Protocol (MCP).

A person creates the workspace and approves the agent. The agent then reads the
controls, submits evidence, maps evidence to controls, and prepares what an
auditor needs. Proofplane records who did each action, and when.

## Who it is for

Proofplane is for founders, CTOs, engineering leaders, and operations leads at
5-50 person B2B SaaS and AI startups. These users answer a customer security
questionnaire, prepare for an audit, or make scattered compliance work
reviewable. They are technical, but compliance is not their full-time job.

Auditors and advisors are secondary users. They receive a scoped, time-limited
access link instead of a full account.

## How it works

Proofplane has two planes.

The **control plane** is a REST API. A person uses it to sign in, to manage the
workspace and its members, to approve an agent connection, and to upload a
document in the browser. It authenticates with Auth0 user tokens.

The **data plane** is an MCP server. An agent uses it to work with compliance
data. There is no REST data plane and no static API key. An agent obtains a
short-lived Proofplane access token through an OAuth flow with Authorization
Code and PKCE. Auth0 is the upstream human login, and the person chooses the
permissions at consent time.

Each user has exactly one workspace. Every read and every write is scoped to it.

## What an agent can do

The MCP server exposes about 40 tools. The main groups are:

- **Frameworks and controls** — list frameworks and their requirements, create a
  control, and read control detail.
- **Evidence** — create an evidence target, submit a file for a coverage window,
  and read the latest submission.
- **Policies** — write a policy, attach a document to it, and archive it.
- **Mappings** — map evidence to controls and policies to controls, in both
  directions.
- **Auditor access** — create a scoped access link for an auditor, list the live
  links, and revoke one.

Every uploaded file is staged first, scanned for malware, and only then
finalized. A file that fails the scan never becomes usable evidence.

## Architecture

Proofplane is a single Rust crate that builds five production commands.

| Command    | Role                                                        |
| ---------- | ----------------------------------------------------------- |
| `api`      | Control-plane REST and the OAuth authorization server        |
| `mcp`      | The MCP data plane, over Streamable HTTP                     |
| `worker`   | Handles messages pushed from Pub/Sub, such as malware scans  |
| `dequeuer` | Publishes the transactional outbox to Pub/Sub                |
| `migrate`  | Applies the database migrations and nothing else             |

The application uses CQRS with complete aggregate snapshots in one Postgres
database. It is not event sourced, and there is no separate read database.
A command mutates an aggregate inside a unit of work and writes its outbox
messages in the same transaction. A query loads a purpose-built read model.

The external boundaries are Postgres for persistence, Google Pub/Sub for
messages, Google Cloud Storage for documents, ClamAV for malware scans, and
Auth0 for human identity. Production runs on Google Cloud Run. Local development
replaces the cloud parts with Docker containers, an emulator, and the
filesystem.

## Status

Proofplane is pre-1.0 and under active development. One known limit: an agent
token lasts 24 hours and there is no refresh token yet, so the agent repeats the
browser flow each day.

## Run it locally

You need Rust, `make`, and Docker. From the repository root:

```bash
cargo build
make up      # Postgres, the Pub/Sub emulator, and ClamAV
make health
make seed    # migrations and demo data
make api     # http://127.0.0.1:3000
make mcp     # http://127.0.0.1:3002/mcp
```

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) for the complete setup. It covers the
config file, the OAuth flow, and how to connect Codex or Claude to your local
server.

## Documentation

- [`CONTRIBUTING.md`](CONTRIBUTING.md) — setup, local processes, and validation.
- [`CLAUDE.md`](CLAUDE.md) — repository layout, architecture rules, and coding
  standards.
- [`CONTEXT.md`](CONTEXT.md) — the domain vocabulary.
- [`PRODUCT.md`](PRODUCT.md) — the product and brand direction.
- [`docs/adr/`](docs/adr) — architectural decisions and the reasons for them.
- [`migrations/README.md`](migrations/README.md) — the expand-then-contract rule
  for every schema change.

## License

Proofplane is released under the MIT License. See [`LICENSE`](LICENSE).
