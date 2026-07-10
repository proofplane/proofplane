import { useAuth0 } from "@auth0/auth0-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Check, Copy, RefreshCw, ShieldCheck } from "lucide-react";
import { FormEvent, ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { ApiError, createApiClient } from "../api/client";
import {
  listAgentConnections,
  revokeAgentConnection,
  type AgentConnection,
  type AgentConnectionsResponse,
} from "../api/agentConnections";
import { createWorkspace, getWorkspace } from "../api/workspaces";
import { getAuthConfig } from "../auth/config";
import { AuthLoading } from "../components/AuthLoading";
import { Button } from "../components/Button";
import { Shell } from "../components/Shell";
import { Skeleton } from "../components/Skeleton";
import { StatusPanel } from "../components/StatusPanel";

export function AppRoute() {
  const config = getAuthConfig();

  if (!config) {
    return (
      <Shell>
        <section className="page-heading" aria-labelledby="app-title">
          <p className="eyebrow">Auth setup</p>
          <h1 id="app-title">Auth0 is not configured.</h1>
          <p>Add the Auth0 Vite environment variables before opening workspace setup.</p>
        </section>
      </Shell>
    );
  }

  return <ConfiguredAppRoute config={config} />;
}

function ConfiguredAppRoute({ config }: { config: NonNullable<ReturnType<typeof getAuthConfig>> }) {
  const { getAccessTokenSilently, isAuthenticated, isLoading, loginWithRedirect } = useAuth0();
  const location = useLocation();
  const loginStarted = useRef(false);
  const apiClient = useMemo(
    () => createApiClient({ getAccessToken: getAccessTokenSilently }),
    [getAccessTokenSilently],
  );
  const returnTo = `${location.pathname}${location.search}${location.hash}`;

  useEffect(() => {
    if (isLoading || isAuthenticated || loginStarted.current) {
      return;
    }

    loginStarted.current = true;
    void loginWithRedirect({
      appState: { returnTo },
      authorizationParams: {
        audience: config.audience,
        redirect_uri: `${window.location.origin}/auth/callback`,
      },
    });
  }, [config.audience, isAuthenticated, isLoading, loginWithRedirect, returnTo]);

  if (isLoading || !isAuthenticated) {
    return (
      <Shell>
        <AuthLoading />
      </Shell>
    );
  }

  return (
    <Shell>
      <Routes>
        <Route index element={<WorkspaceGate apiClient={apiClient} />} />
        <Route path="onboarding" element={<WorkspaceOnboarding apiClient={apiClient} />} />
        <Route path="connect" element={<WorkspaceConnectionRoute apiClient={apiClient} />} />
      </Routes>
    </Shell>
  );
}

type WorkspaceRouteProps = {
  apiClient: ReturnType<typeof createApiClient>;
};

function WorkspaceGate({ apiClient }: WorkspaceRouteProps) {
  const workspace = useWorkspace(apiClient);

  if (workspace.isLoading) {
    return <WorkspaceSkeleton />;
  }

  if (workspace.isError) {
    return <WorkspaceError error={workspace.error} />;
  }

  if (!workspace.data) {
    return <Navigate replace to="/app/onboarding" />;
  }

  return <Navigate replace to="/app/connect" />;
}

function WorkspaceOnboarding({ apiClient }: WorkspaceRouteProps) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const workspace = useWorkspace(apiClient);

  const create = useMutation({
    mutationFn: () => createWorkspace(apiClient, { name: name.trim() }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: workspaceQueryKey });
      navigate("/app/connect");
    },
  });

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    create.mutate();
  }

  if (workspace.isLoading) {
    return <WorkspaceSkeleton />;
  }

  if (workspace.isError) {
    return <WorkspaceError error={workspace.error} />;
  }

  if (workspace.data) {
    return <Navigate replace to="/app/connect" />;
  }

  return (
    <>
      <section className="page-heading onboarding-heading" aria-labelledby="app-title">
        <p className="eyebrow">Workspace setup</p>
        <h1 id="app-title">Create a workspace.</h1>
        <p>Name the workspace. Proofplane will create it and make you the owner.</p>
      </section>

      <form className="onboarding-form" onSubmit={submit}>
        <label className="field">
          <span>Workspace name</span>
          <input
            autoComplete="organization"
            name="name"
            onChange={(event) => setName(event.target.value)}
            required
            value={name}
          />
        </label>

        {create.isError ? <FormError error={create.error} /> : null}

        <Button disabled={create.isPending || !name.trim()} type="submit">
          {create.isPending ? "Creating workspace" : "Create workspace"}
          <ArrowRight aria-hidden="true" size={16} />
        </Button>
      </form>
    </>
  );
}

function WorkspaceConnectionRoute({ apiClient }: WorkspaceRouteProps) {
  const workspace = useWorkspace(apiClient);

  if (workspace.isLoading) {
    return <WorkspaceSkeleton />;
  }

  if (workspace.isError) {
    return <WorkspaceError error={workspace.error} />;
  }

  if (!workspace.data) {
    return <Navigate replace to="/app/onboarding" />;
  }

  return <ConnectionManagement apiClient={apiClient} />;
}

const connectionsQueryKey = ["agent-connections"];
const firstPrompt =
  "Review my SOC 2 readiness. Start by listing the highest-priority evidence gaps and the next action for each.";

function ConnectionManagement({ apiClient }: WorkspaceRouteProps) {
  const queryClient = useQueryClient();
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [revokedClient, setRevokedClient] = useState<string | null>(null);
  const connections = useQuery({
    queryFn: () => listAgentConnections(apiClient),
    queryKey: connectionsQueryKey,
    retry: false,
  });
  const revoke = useMutation({
    mutationFn: (connection: AgentConnection) => revokeAgentConnection(apiClient, connection.id),
    onSuccess: (_, connection) => {
      queryClient.setQueryData<AgentConnectionsResponse>(connectionsQueryKey, (current) =>
        current
          ? {
            ...current,
            connections: current.connections.filter((candidate) => candidate.id !== connection.id),
          }
          : current,
      );
      setConfirmingId(null);
      setRevokedClient(connection.client_name);
    },
  });

  if (connections.isLoading) {
    return <ConnectionManagementSkeleton />;
  }

  if (connections.isError) {
    return (
      <StatusPanel title="Connections did not load" tone="preview">
        <p>{friendlyError(connections.error)}</p>
        <Button onClick={() => void connections.refetch()} variant="secondary">Try again</Button>
      </StatusPanel>
    );
  }

  if (!connections.data) {
    return (
      <StatusPanel title="Connections did not load" tone="preview">
        <p>Proofplane did not return connection details. Try again.</p>
        <Button onClick={() => void connections.refetch()} variant="secondary">Try again</Button>
      </StatusPanel>
    );
  }

  const data = connections.data;

  return (
    <>
      <section className="page-heading connection-heading" aria-labelledby="connect-title">
        <p className="eyebrow">Agent connections</p>
        <h1 id="connect-title">Connect Proofplane.</h1>
        <p>Choose your client, add the hosted MCP server, and approve access in your browser.</p>
      </section>

      <CopyBlock label="Proofplane MCP URL" value={data.mcp_url} />

      <section className="setup-grid" aria-label="Guided client setup">
        <SetupCard
          title="Claude Desktop"
          steps={[
            "Open Customize → Connectors.",
            "Select + or Add, then Add custom connector.",
            "Name it Proofplane, paste the MCP URL, add the connector, and select Connect.",
            "In the browser, select Grant access.",
          ]}
        />
        <SetupCard
          title="ChatGPT Desktop"
          steps={[
            "Open Settings → Plugins and select MCPs",
            "Select Add server and choose Streamable HTTP.",
            <>
              Name it Proofplane, paste the MCP URL, save, then select the
              <RefreshCw
                aria-label="Refresh icon"
                className="setup-refresh-icon"
                role="img"
                size={18}
              />
              button.
            </>,
            "Select Authenticate and, in the browser, select Grant access.",
          ]}
        />
      </section>

      <section className="reconnect-note" aria-labelledby="reconnect-title">
        <ShieldCheck aria-hidden="true" size={22} />
        <div>
          <h2 id="reconnect-title">Reconnect after 24 hours</h2>
          <p>If your client asks you to authenticate again, return to its connector or MCP server settings and select Connect or Authenticate.</p>
        </div>
      </section>

      <CopyBlock label="First SOC 2 prompt" multiline value={firstPrompt} />

      <section className="connections-panel" aria-labelledby="connections-title">
        <div className="connections-header">
          <div>
            <h2 id="connections-title">Your connections</h2>
          </div>
        </div>

        {revokedClient ? (
          <p className="revoke-success" role="status">
            Access for {revokedClient} was revoked. You can also remove Proofplane from that client’s settings.
          </p>
        ) : null}

        {data.connections.length === 0 ? (
          <p className="connections-empty">No active connections yet. Follow either verified setup above to connect.</p>
        ) : (
          <div className="connection-list">
            {data.connections.map((connection) => (
              <ConnectionRow
                connection={connection}
                confirming={confirmingId === connection.id}
                key={connection.id}
                onCancel={() => {
                  setConfirmingId(null);
                  revoke.reset();
                }}
                onConfirm={() => revoke.mutate(connection)}
                onRevoke={() => {
                  setRevokedClient(null);
                  revoke.reset();
                  setConfirmingId(connection.id);
                }}
                pending={revoke.isPending && confirmingId === connection.id}
                revokeError={revoke.isError && confirmingId === connection.id ? revoke.error : null}
              />
            ))}
          </div>
        )}
      </section>
    </>
  );
}

function SetupCard({ title, steps }: { title: string; steps: ReactNode[] }) {
  return (
    <article className="setup-card">
      <span className="capability-label"><Check aria-hidden="true" size={14} /> Verified setup</span>
      <h2>{title}</h2>
      <ol>{steps.map((step, index) => <li key={index}>{step}</li>)}</ol>
    </article>
  );
}

function CopyBlock({ label, multiline = false, value }: { label: string; multiline?: boolean; value: string }) {
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setCopyError(false);
    } catch {
      setCopyError(true);
      setCopied(false);
    }
  }

  return (
    <section className="connection-copy-block">
      <div className="copy-block-header">
        <span>{label}</span>
        <Button aria-label={`Copy ${label}`} onClick={() => void copy()} variant="secondary">
          {copied ? <Check aria-hidden="true" size={16} /> : <Copy aria-hidden="true" size={16} />}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <code className={multiline ? "copy-value copy-value-multiline" : "copy-value"}>{value}</code>
      {copyError ? <p className="form-error" role="alert">Copy failed. Select the text and copy it manually.</p> : null}
    </section>
  );
}

function ConnectionRow({
  connection,
  confirming,
  onCancel,
  onConfirm,
  onRevoke,
  pending,
  revokeError,
}: {
  connection: AgentConnection;
  confirming: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  onRevoke: () => void;
  pending: boolean;
  revokeError: Error | null;
}) {
  return (
    <article className="connection-row">
      <div className="connection-row-main">
        <div>
          <div className="connection-title-line">
            <h3>{connection.client_name}</h3>
            <span className="connection-status">{connection.status === "active" ? "Used" : "Access granted"}</span>
          </div>
          <dl className="connection-times">
            <div><dt>Authorized</dt><dd>{formatDate(connection.authorized_at)}</dd></div>
            <div><dt>Last used</dt><dd>{connection.last_used_at ? formatDate(connection.last_used_at) : "Not yet"}</dd></div>
          </dl>
        </div>
        {!confirming ? <Button onClick={onRevoke} variant="secondary">Revoke</Button> : null}
      </div>
      {confirming ? (
        <div className="revoke-confirmation">
          <p>Revoke {connection.client_name} access to Proofplane?</p>
          <div className="revoke-actions">
            <Button disabled={pending} onClick={onConfirm}>{pending ? "Revoking" : "Confirm revoke"}</Button>
            <Button disabled={pending} onClick={onCancel} variant="secondary">Cancel</Button>
          </div>
          {revokeError ? <p className="form-error" role="alert">{friendlyError(revokeError)} The connection is still active.</p> : null}
        </div>
      ) : null}
    </article>
  );
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value));
}

function ConnectionManagementSkeleton() {
  return (
    <section aria-label="Loading connections" className="connections-loading">
      <Skeleton className="skeleton-heading" />
      <Skeleton className="skeleton-copy" />
      <Skeleton className="skeleton-connection-card" />
      <Skeleton className="skeleton-connection-card" />
    </section>
  );
}

function WorkspaceError({ error }: { error: Error }) {
  return (
    <StatusPanel title="Workspace did not load" tone="preview">
      <p>{friendlyError(error)}</p>
    </StatusPanel>
  );
}

function WorkspaceSkeleton() {
  return (
    <>
      <section className="page-heading onboarding-heading" aria-label="Loading workspace">
        <Skeleton className="skeleton-eyebrow" />
        <Skeleton className="skeleton-heading" />
        <Skeleton className="skeleton-copy" />
      </section>

      <div className="workspace-list" aria-hidden="true">
        <SkeletonWorkspaceRow />
        <SkeletonWorkspaceRow />
      </div>
    </>
  );
}

function SkeletonWorkspaceRow() {
  return (
    <article className="workspace-row skeleton-row">
      <div>
        <Skeleton className="skeleton-title" />
        <Skeleton className="skeleton-meta" />
      </div>
      <Skeleton className="skeleton-button" />
    </article>
  );
}

function FormError({ error }: { error: Error }) {
  return (
    <p className="form-error" role="alert">
      {friendlyError(error)}
    </p>
  );
}

function friendlyError(error: Error) {
  if (error instanceof ApiError && error.status === 409) {
    return error.message || "This user already belongs to a workspace.";
  }

  if (error instanceof ApiError && error.status === 401) {
    return "You do not have access to this workspace action.";
  }

  if (error instanceof TypeError) {
    return "Proofplane could not reach the API. Check that the backend is running and accepting browser requests.";
  }

  return error.message || "Proofplane could not complete this request.";
}

function useWorkspace(apiClient: ReturnType<typeof createApiClient>) {
  return useQuery({
    queryFn: () => getWorkspace(apiClient),
    queryKey: workspaceQueryKey,
    retry: false,
  });
}

const workspaceQueryKey = ["workspace"];
