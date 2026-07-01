import { useAuth0 } from "@auth0/auth0-react";
import { useEffect, useMemo, useState } from "react";
import { useLocation } from "react-router-dom";
import { createApiClient } from "../api/client";
import { listWorkspaces, type Workspace } from "../api/workspaces";
import { getAuthConfig } from "../auth/config";
import { Button } from "../components/Button";
import { Shell } from "../components/Shell";

type RequestView = { id: string; client_name: string; scopes: string[]; expires_at: string };
type Decision = { redirect_uri: string };

export function McpAuthorizeRoute() {
  const auth = useAuth0();
  const route = useLocation();
  const config = getAuthConfig();
  const requestId = new URLSearchParams(route.search).get("request_id");
  const client = useMemo(
    () => createApiClient({ getAccessToken: auth.getAccessTokenSilently }),
    [auth.getAccessTokenSilently],
  );
  const [request, setRequest] = useState<RequestView>();
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [workspaceId, setWorkspaceId] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    if (!config || auth.isLoading || auth.isAuthenticated || !requestId) return;
    void auth.loginWithRedirect({ appState: { returnTo: `${route.pathname}${route.search}` } });
  }, [auth, config, requestId, route.pathname, route.search]);

  useEffect(() => {
    if (!auth.isAuthenticated || !requestId) return;
    void Promise.all([
      client.request<RequestView>(`/oauth/requests/${requestId}`),
      listWorkspaces(client),
    ]).then(([pending, available]) => {
      setRequest(pending);
      setWorkspaces(available);
      setWorkspaceId(available[0]?.id ?? "");
    }).catch(() => setError("This authorization request is invalid or expired."));
  }, [auth.isAuthenticated, client, requestId]);

  async function decide(action: "approve" | "deny") {
    if (!requestId) return;
    try {
      const decision = await client.request<Decision>(`/oauth/requests/${requestId}/${action}`, {
        method: "POST",
        headers: action === "approve" ? { "Content-Type": "application/json" } : undefined,
        body: action === "approve" ? JSON.stringify({ workspace_id: workspaceId }) : undefined,
      });
      location.assign(decision.redirect_uri);
    } catch {
      setError("Authorization could not be completed.");
    }
  }

  return <Shell><main className="page-heading">
    <p className="eyebrow">Agent connection</p>
    <h1>Authorize {request?.client_name ?? "MCP client"}</h1>
    {error ? <p role="alert">{error}</p> : <>
      <label>Workspace <select value={workspaceId} onChange={(event) => setWorkspaceId(event.target.value)}>
        {workspaces.map((workspace) => <option key={workspace.id} value={workspace.id}>{workspace.name}</option>)}
      </select></label>
      <p>Permissions: {request?.scopes.filter((scope) => scope !== "offline_access").join(", ")}</p>
      <Button disabled={!request || !workspaceId} onClick={() => void decide("approve")}>Approve</Button>
      <Button disabled={!request} onClick={() => void decide("deny")}>Deny</Button>
    </>}
  </main></Shell>;
}
