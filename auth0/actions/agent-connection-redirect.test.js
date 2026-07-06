"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const action = require("./agent-connection-redirect");

const RESOURCE = "https://mcp.proofplane.test/mcp";
const SCOPES = ["read_evidence_requests", "write_controls"];

function event(overrides = {}) {
  return {
    secrets: {
      PROOFPLANE_API_BASE_URL: "https://api.proofplane.test",
      PROOFPLANE_MCP_RESOURCE: RESOURCE,
      PROOFPLANE_ALLOWED_CLIENT_IDS: "allowed-client,second-client",
      PROOFPLANE_ACTION_SHARED_SECRET: "01234567890123456789012345678901",
    },
    resource_server: { identifier: RESOURCE },
    client: { client_id: "allowed-client", name: "Agent Client" },
    user: { user_id: "auth0|user" },
    transaction: {
      id: "transaction",
      state: "oauth-state",
      requested_scopes: SCOPES,
    },
    request: {
      hostname: "tenant.auth0.com",
      query: { state: "auth0-redirect-state", session_token: "result" },
    },
    ...overrides,
  };
}

function api(result = {}) {
  const calls = { claims: [], redirects: [], denied: [], encoded: [] };
  return {
    calls,
    accessToken: {
      setCustomClaim(name, value) {
        calls.claims.push([name, value]);
      },
    },
    access: {
      deny(reason) {
        calls.denied.push(reason);
      },
    },
    redirect: {
      encodeToken(options) {
        calls.encoded.push(options);
        return "input-token";
      },
      sendUserTo(url, options) {
        calls.redirects.push([url, options]);
      },
      validateToken() {
        return result;
      },
    },
  };
}

function response(body, ok = true) {
  return { ok, async json() { return body; } };
}

test.afterEach(() => {
  delete global.fetch;
});

test("ignores authorization for an unrelated resource", async () => {
  global.fetch = () => { throw new Error("must not fetch"); };
  const actionApi = api();
  await action.onExecutePostLogin(
    event({ resource_server: { identifier: "https://other.example/" } }),
    actionApi,
  );
  assert.deepEqual(actionApi.calls, {
    claims: [], redirects: [], denied: [], encoded: [],
  });
});

test("reusable connection injects both namespaced claims without redirect", async () => {
  global.fetch = async () =>
    response({
      outcome: "reusable",
      connection_id: "connection",
      workspace_id: "workspace",
      scopes: SCOPES,
    });
  const actionApi = api();
  await action.onExecutePostLogin(event(), actionApi);
  assert.deepEqual(actionApi.calls.claims, [
    ["https://proofplane.com/connection_id", "connection"],
    ["https://proofplane.com/workspace_id", "workspace"],
  ]);
  assert.equal(actionApi.calls.redirects.length, 0);
});

test("missing reuse invokes Auth0 redirect even for prompt none", async () => {
  global.fetch = async () => response({ outcome: "interaction_required" });
  const actionApi = api();
  const silent = event();
  silent.request.query.prompt = "none";
  await action.onExecutePostLogin(silent, actionApi);
  assert.equal(actionApi.calls.redirects.length, 1);
  assert.equal(actionApi.calls.encoded[0].expiresInSeconds, 300);
  assert.equal(actionApi.calls.encoded[0].payload.oauth_state, "oauth-state");
});

test("approved continuation is consumed and compared before claims are set", async () => {
  global.fetch = async () =>
    response({
      outcome: "approved",
      connection_id: "connection",
      workspace_id: "workspace",
      subject: "auth0|user",
      client_id: "allowed-client",
      resource: RESOURCE,
      scopes: SCOPES,
    });
  const actionApi = api({
    purpose: "proofplane_agent_connection_result",
    version: 1,
    decision: "approved",
    sub: "auth0|user",
    transaction_id: "transaction",
    oauth_state: "oauth-state",
    continuation_token: "continuation",
    nonce: "nonce",
    iss: "https://api.proofplane.test/agent-connections/consent",
    aud: "https://tenant.auth0.com/",
  });
  await action.onContinuePostLogin(event(), actionApi);
  assert.equal(actionApi.calls.claims.length, 2);
  assert.deepEqual(actionApi.calls.denied, []);
});

test("denial and malformed or mismatched results deny access", async () => {
  for (const result of [
    {
      purpose: "proofplane_agent_connection_result",
      version: 1,
      decision: "denied",
      sub: "auth0|user",
      transaction_id: "transaction",
      oauth_state: "oauth-state",
      iss: "https://api.proofplane.test/agent-connections/consent",
      aud: "https://tenant.auth0.com/",
    },
    { decision: "approved" },
    {
      purpose: "proofplane_agent_connection_result",
      version: 1,
      decision: "approved",
      sub: "auth0|other",
      transaction_id: "transaction",
      oauth_state: "oauth-state",
      continuation_token: "continuation",
      nonce: "nonce",
      iss: "https://api.proofplane.test/agent-connections/consent",
      aud: "https://tenant.auth0.com/",
    },
  ]) {
    global.fetch = async () => response({ outcome: "invalid_continuation" });
    const actionApi = api(result);
    await action.onContinuePostLogin(event(), actionApi);
    assert.deepEqual(actionApi.calls.denied, ["access_denied"]);
    assert.deepEqual(actionApi.calls.claims, []);
  }
});

test("continuation replay and API failure deny access", async () => {
  const valid = {
    purpose: "proofplane_agent_connection_result",
    version: 1,
    decision: "approved",
    sub: "auth0|user",
    transaction_id: "transaction",
    oauth_state: "oauth-state",
    continuation_token: "continuation",
    nonce: "nonce",
    iss: "https://api.proofplane.test/agent-connections/consent",
    aud: "https://tenant.auth0.com/",
  };
  for (const apiResponse of [
    response({ outcome: "invalid_continuation" }),
    response({}, false),
  ]) {
    global.fetch = async () => apiResponse;
    const actionApi = api(valid);
    await action.onContinuePostLogin(event(), actionApi);
    assert.deepEqual(actionApi.calls.denied, ["access_denied"]);
  }
});
