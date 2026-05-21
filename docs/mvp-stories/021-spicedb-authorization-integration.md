# 021 - SpiceDB Authorization Integration

## Goal

Integrate SpiceDB as the fine-grained authorization backend after Proofplane has
enough domain entities and actions to model the permission graph with real
product pressure.

## Design

Story 010 owns authentication, actor context, a narrow workspace-scoped
authorization boundary for Evidence Requests, and a bounded SpiceDB design
spike. This story turns that later spike output into a production authorization
backend once controls, mappings, submissions, approvals, source material, and
audit behavior have clarified the authorization model.

Use the authorization boundary introduced in story 010 rather than spreading
SpiceDB calls through route handlers or repositories. Keep authentication in
Proofplane: credentials resolve to actor context before authorization checks are
made.

The story should refine and implement:

- the SpiceDB schema for Proofplane subjects, workspaces, domain resources,
  relations, and permissions
- relationship write and synchronization strategy for domain changes
- local and test runtime configuration for the SpiceDB service
- consistency behavior for permission changes that must affect immediate reads
  or writes
- list-query authorization strategy for API and MCP reads
- migration path from the initial local authorization implementation

The schema should be driven by concrete MVP actions, including Evidence Request
reads/writes, mapping changes, submission actions, approvals, source-material
access, and audit inspection. Avoid a generic role model that is not exercised
by those actions.

## Acceptance Criteria

- Proofplane has a SpiceDB-backed implementation of the authorization boundary.
- The SpiceDB schema covers the MVP actors, workspace relations, protected
  resources, and concrete permissions identified by stories 010 and 016-020.
- Domain changes that affect authorization update or synchronize SpiceDB
  relationships through a documented and tested strategy.
- API authorization behavior remains stable when switching from the initial
  authorization implementation to SpiceDB.
- List endpoints use a documented authorization strategy rather than loading all
  rows and filtering ad hoc.
- Local development and integration-test setup can run the required SpiceDB
  checks deterministically.

## Tests

- Schema tests cover representative allowed and denied relationship paths.
- Authorization boundary tests run against the SpiceDB-backed implementation.
- API integration tests cover representative protected reads and writes across
  workspaces and domain resource types.
- Synchronization tests cover relationship creation, updates, and removals for
  authorization-relevant domain changes.
- List-query tests cover authorized and unauthorized resources without leaking
  cross-workspace data.

## QA Guide

1. Start local dependencies including SpiceDB.
2. Seed actors, workspaces, and representative domain resources.
3. Exercise representative read, write, approval, and audit permissions with
   allowed and denied actors.
4. Change an authorization-relevant relationship and verify the next affected
   operation observes the intended permission behavior.
5. Confirm API and MCP callers share the same authorization decisions where they
   expose the same domain action.
