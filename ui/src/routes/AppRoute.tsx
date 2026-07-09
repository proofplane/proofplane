import { useAuth0 } from "@auth0/auth0-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Bot, CheckCircle2, PlugZap } from "lucide-react";
import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { Navigate, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { ApiError, createApiClient } from "../api/client";
import { createWorkspace, getWorkspace, type Workspace } from "../api/workspaces";
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

  return <WorkspaceConnection workspace={workspace.data} />;
}

function WorkspaceConnection({ workspace }: { workspace: Workspace }) {
  return (
    <>
      <section className="page-heading token-heading" aria-labelledby="connect-title">
        <p className="eyebrow">MCP connection</p>
        <h1 id="connect-title">Connect an MCP client.</h1>
        <p>
          {workspace.name} is ready. Use an MCP client that supports OAuth; Proofplane will
          ask you to approve access for this workspace during connection.
        </p>
      </section>

      <section className="workspace-connect-panel" aria-labelledby="workspace-summary-title">
        <div>
          <h2 id="workspace-summary-title">{workspace.name}</h2>
          <p>Current role: {workspace.role}</p>
        </div>
        <CheckCircle2 aria-hidden="true" size={24} />
      </section>

      <section className="workspace-connect-grid" aria-label="Connection steps">
        <article>
          <PlugZap aria-hidden="true" size={22} />
          <h2>Start from your MCP client</h2>
          <p>Choose Proofplane as the server and complete the browser authorization flow.</p>
        </article>
        <article>
          <Bot aria-hidden="true" size={22} />
          <h2>Approve requested access</h2>
          <p>The consent page shows this workspace and the exact permissions requested.</p>
        </article>
      </section>
    </>
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
