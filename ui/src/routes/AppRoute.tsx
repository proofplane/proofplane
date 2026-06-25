import { useAuth0 } from "@auth0/auth0-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight } from "lucide-react";
import { FormEvent, useMemo, useState } from "react";
import { Link, Navigate, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { ApiError, createApiClient } from "../api/client";
import { createWorkspace, listWorkspaces, type Workspace } from "../api/workspaces";
import { getAuthConfig } from "../auth/config";
import { StartWorkspaceButton } from "../auth/StartWorkspaceButton";
import { Button } from "../components/Button";
import { Shell } from "../components/Shell";
import { StatusPanel } from "../components/StatusPanel";

export function AppRoute() {
  const config = getAuthConfig();

  if (!config) {
    return (
      <Shell>
        <section className="page-heading" aria-labelledby="app-title">
          <p className="eyebrow">Auth setup</p>
          <h1 id="app-title">Auth0 is not configured.</h1>
          <p>Add the Auth0 Vite environment variables before opening the workspace flow.</p>
        </section>
      </Shell>
    );
  }

  return <ConfiguredAppRoute />;
}

function ConfiguredAppRoute() {
  const { getAccessTokenSilently, isAuthenticated, isLoading } = useAuth0();
  const apiClient = useMemo(
    () => createApiClient({ getAccessToken: getAccessTokenSilently }),
    [getAccessTokenSilently],
  );

  if (isLoading) {
    return (
      <Shell>
        <StatusPanel title="Opening workspace flow">Checking your Auth0 session.</StatusPanel>
      </Shell>
    );
  }

  if (!isAuthenticated) {
    return (
      <Shell>
        <section className="page-heading" aria-labelledby="app-title">
          <p className="eyebrow">Sign in required</p>
          <h1 id="app-title">Start with Auth0.</h1>
          <p>Proofplane needs an authenticated user before creating or resuming a workspace.</p>
          <div className="actions">
            <StartWorkspaceButton />
          </div>
        </section>
      </Shell>
    );
  }

  return (
    <Shell>
      <Routes>
        <Route index element={<WorkspaceGate apiClient={apiClient} />} />
        <Route path="onboarding" element={<WorkspaceOnboarding apiClient={apiClient} />} />
        <Route path="workspaces/:workspaceId/tokens" element={<TokenRoutePlaceholder />} />
      </Routes>
    </Shell>
  );
}

type WorkspaceRouteProps = {
  apiClient: ReturnType<typeof createApiClient>;
};

function WorkspaceGate({ apiClient }: WorkspaceRouteProps) {
  const workspaces = useWorkspaces(apiClient);

  if (workspaces.isLoading) {
    return <StatusPanel title="Loading workspaces">Checking for existing workspaces.</StatusPanel>;
  }

  if (workspaces.isError) {
    return <WorkspaceError error={workspaces.error} />;
  }

  if (!workspaces.data?.length) {
    return <Navigate replace to="/app/onboarding" />;
  }

  return <WorkspaceResume workspaces={workspaces.data} />;
}

function WorkspaceOnboarding({ apiClient }: WorkspaceRouteProps) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");

  const create = useMutation({
    mutationFn: () => createWorkspace(apiClient, { name: name.trim() }),
    onSuccess: async (workspace) => {
      await queryClient.invalidateQueries({ queryKey: ["workspaces"] });
      navigate(`/app/workspaces/${workspace.id}/tokens`);
    },
  });

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    create.mutate();
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

function WorkspaceResume({ workspaces }: { workspaces: Workspace[] }) {
  return (
    <>
      <section className="page-heading onboarding-heading" aria-labelledby="app-title">
        <p className="eyebrow">Workspace setup</p>
        <h1 id="app-title">Resume a workspace.</h1>
        <p>Select an existing workspace instead of creating a duplicate.</p>
      </section>

      <div className="workspace-list">
        {workspaces.map((workspace) => (
          <article className="workspace-row" key={workspace.id}>
            <div>
              <h2>{workspace.name}</h2>
              <p>
                {workspace.role} · {workspace.id}
              </p>
            </div>
            <Link className="button button-primary" to={`/app/workspaces/${workspace.id}/tokens`}>
              Resume setup
              <ArrowRight aria-hidden="true" size={16} />
            </Link>
          </article>
        ))}
      </div>

      <Link className="button button-secondary onboarding-secondary-action" to="/app/onboarding">
        Create another workspace
      </Link>
    </>
  );
}

function TokenRoutePlaceholder() {
  const { workspaceId } = useParams();

  return (
    <section className="page-heading" aria-labelledby="tokens-title">
      <p className="eyebrow">Token setup</p>
      <h1 id="tokens-title">Token creation is next.</h1>
      <p>Workspace {workspaceId} is ready for the scoped token flow.</p>
    </section>
  );
}

function WorkspaceError({ error }: { error: Error }) {
  return (
    <StatusPanel title="Workspaces did not load" tone="preview">
      <p>{friendlyError(error)}</p>
    </StatusPanel>
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
    return error.message || "A workspace with those details already exists.";
  }

  if (error instanceof ApiError && (error.status === 401 || error.status === 404)) {
    return "You do not have access to this workspace action.";
  }

  if (error instanceof TypeError) {
    return "Proofplane could not reach the API. Check that the backend is running and accepting browser requests.";
  }

  return error.message || "Proofplane could not complete this request.";
}

function useWorkspaces(apiClient: ReturnType<typeof createApiClient>) {
  return useQuery({
    queryFn: () => listWorkspaces(apiClient),
    queryKey: ["workspaces"],
    retry: false,
  });
}
