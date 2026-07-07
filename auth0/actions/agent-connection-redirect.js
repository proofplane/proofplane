"use strict";

const INPUT_PURPOSE = "proofplane_agent_connection_consent";
const RESULT_PURPOSE = "proofplane_agent_connection_result";
const TOKEN_VERSION = 1;
const CONNECTION_CLAIM = "https://proofplane.com/connection_id";
const WORKSPACE_CLAIM = "https://proofplane.com/workspace_id";
const KNOWN_SCOPES = [
  "read_evidence_requests",
  "write_evidence_requests",
  "read_evidence_submissions",
  "write_evidence_submissions",
  "read_controls",
  "write_controls",
];
const IGNORED_OIDC_SCOPES = new Set([
  "openid",
  "profile",
  "offline_access",
  "name",
  "given_name",
  "family_name",
  "nickname",
  "email",
  "email_verified",
  "picture",
  "created_at",
  "identities",
  "phone",
  "address",
]);

exports.onExecutePostLogin = async (event, api) => {
  const config = readConfig(event);
  const resource = event.resource_server && event.resource_server.identifier;
  if (resource !== config.resource) return;

  try {
    const transaction = activeTransaction(event, config);
    const resolution = await postJson(
      `${config.apiBaseUrl}/internal/auth0-actions/agent-connections/resolve`,
      config.sharedSecret,
      {
        subject: transaction.sub,
        client_id: transaction.clientId,
        resource: transaction.resource,
        scopes: transaction.scopes,
      },
    );

    if (resolution.outcome === "reusable") {
      requireExactScopes(resolution.scopes, transaction.scopes);
      normalizeAccessTokenScopes(api, transaction);
      setClaims(api, resolution.connection_id, resolution.workspace_id);
      return;
    }
    if (resolution.outcome !== "interaction_required") {
      throw new Error("unexpected resolution");
    }

    const sessionToken = api.redirect.encodeToken({
      secret: config.sharedSecret,
      expiresInSeconds: 300,
      payload: {
        purpose: INPUT_PURPOSE,
        version: TOKEN_VERSION,
        transaction_id: transaction.transactionId,
        oauth_state: transaction.oauthState,
        client_id: transaction.clientId,
        client_name: transaction.clientName,
        resource: transaction.resource,
        scopes: transaction.scopes,
        sub: transaction.sub,
        aud: config.consentUrl,
        iat: Math.floor(Date.now() / 1000),
      },
    });
    // Auth0 itself converts a redirect attempted during prompt=none into
    // interaction_required. Calling the redirect API preserves that native behavior.
    api.redirect.sendUserTo(config.consentUrl, {
      query: { session_token: sessionToken },
    });
  } catch (_error) {
    api.access.deny("access_denied");
  }
};

exports.onContinuePostLogin = async (event, api) => {
  const config = readConfig(event);
  const resource = event.resource_server && event.resource_server.identifier;
  if (resource !== config.resource) return;

  try {
    const transaction = activeTransaction(event, config);
    const result = api.redirect.validateToken({
      secret: config.sharedSecret,
      tokenParameterName: "session_token",
    });
    validateResult(result, transaction, config);
    if (result.decision === "denied") {
      api.access.deny("access_denied");
      return;
    }

    const consumed = await postJson(
      `${config.apiBaseUrl}/internal/auth0-actions/agent-connections/continuations/consume`,
      config.sharedSecret,
      {
        continuation_token: result.continuation_token,
        nonce: result.nonce,
      },
    );
    if (
      consumed.outcome !== "approved" ||
      consumed.subject !== transaction.sub ||
      consumed.client_id !== transaction.clientId ||
      consumed.resource !== transaction.resource
    ) {
      throw new Error("continuation mismatch");
    }
    requireExactScopes(consumed.scopes, transaction.scopes);
    normalizeAccessTokenScopes(api, transaction);
    setClaims(api, consumed.connection_id, consumed.workspace_id);
  } catch (_error) {
    api.access.deny("access_denied");
  }
};

function readConfig(event) {
  const apiBaseUrl = required(event.secrets.PROOFPLANE_API_BASE_URL).replace(
    /\/+$/,
    "",
  );
  const resource = required(event.secrets.PROOFPLANE_MCP_RESOURCE);
  const sharedSecret = required(event.secrets.PROOFPLANE_ACTION_SHARED_SECRET);
  return {
    apiBaseUrl,
    consentUrl: `${apiBaseUrl}/agent-connections/consent`,
    resource,
    sharedSecret,
  };
}

function activeTransaction(event, config) {
  const clientId = required(event.client && event.client.client_id);
  const requestedScopes = (event.transaction && event.transaction.requested_scopes) || [];
  const scopes = requestedMcpScopes(requestedScopes);
  return {
    sub: required(event.user && event.user.user_id),
    clientId,
    clientName: required(
      (event.client && (event.client.name || event.client.client_id)) || "",
    ),
    resource: config.resource,
    scopes,
    requestedScopes,
    transactionId: required(event.transaction && event.transaction.id),
    oauthState: required(event.transaction && event.transaction.state),
    issuer: `https://${required(event.request && event.request.hostname)}/`,
  };
}

function validateResult(result, transaction, config) {
  if (
    !result ||
    result.purpose !== RESULT_PURPOSE ||
    result.version !== TOKEN_VERSION ||
    result.iss !== config.consentUrl ||
    result.aud !== transaction.issuer ||
    result.sub !== transaction.sub ||
    result.transaction_id !== transaction.transactionId ||
    result.oauth_state !== transaction.oauthState ||
    !["approved", "denied"].includes(result.decision)
  ) {
    throw new Error("invalid result");
  }
  if (
    result.decision === "approved" &&
    (!required(result.continuation_token) || !required(result.nonce))
  ) {
    throw new Error("missing continuation");
  }
  if (
    result.decision === "denied" &&
    (result.continuation_token !== undefined || result.nonce !== undefined)
  ) {
    throw new Error("denied result contains secrets");
  }
}

function canonicalScopes(scopes) {
  if (!Array.isArray(scopes) || scopes.length === 0) {
    throw new Error("missing scopes");
  }
  const unique = new Set(scopes);
  if (unique.size !== scopes.length || scopes.some((scope) => !KNOWN_SCOPES.includes(scope))) {
    throw new Error("invalid scopes");
  }
  return KNOWN_SCOPES.filter((scope) => unique.has(scope));
}

function requestedMcpScopes(scopes) {
  if (!Array.isArray(scopes) || scopes.length === 0) {
    throw new Error("missing scopes");
  }
  const unique = new Set(scopes);
  if (unique.size !== scopes.length) {
    throw new Error("invalid scopes");
  }
  const unknown = scopes.filter(
    (scope) => !KNOWN_SCOPES.includes(scope) && !IGNORED_OIDC_SCOPES.has(scope),
  );
  if (unknown.length > 0) {
    throw new Error("invalid scopes");
  }
  const requested = KNOWN_SCOPES.filter((scope) => unique.has(scope));
  return requested.length > 0 ? requested : KNOWN_SCOPES.slice();
}

function requireExactScopes(actual, expected) {
  const canonicalActual = canonicalScopes(actual);
  if (
    canonicalActual.length !== expected.length ||
    canonicalActual.some((scope, index) => scope !== expected[index])
  ) {
    throw new Error("scope mismatch");
  }
}

function normalizeAccessTokenScopes(api, transaction) {
  for (const scope of transaction.requestedScopes) {
    if (!transaction.scopes.includes(scope)) {
      api.accessToken.removeScope(scope);
    }
  }
  for (const scope of transaction.scopes) {
    api.accessToken.addScope(scope);
  }
}

async function postJson(url, secret, body) {
  const response = await fetch(url, {
    method: "POST",
    headers: {
      authorization: `Bearer ${secret}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(5000),
  });
  if (!response.ok) throw new Error("Proofplane API failed");
  return response.json();
}

function setClaims(api, connectionId, workspaceId) {
  api.accessToken.setCustomClaim(CONNECTION_CLAIM, required(connectionId));
  api.accessToken.setCustomClaim(WORKSPACE_CLAIM, required(workspaceId));
}

function required(value) {
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error("required value missing");
  }
  return value;
}
