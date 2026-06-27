import { useAuth0 } from "@auth0/auth0-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight } from "lucide-react";
import { FormEvent, useMemo, useState } from "react";
import { Navigate, Route, Routes, useNavigate, useParams } from "react-router-dom";
import { ApiError, createApiClient } from "../api/client";
import { createWorkspace, listWorkspaces, type Workspace } from "../api/workspaces";
import { getAuthConfig } from "../auth/config";
import { StartWorkspaceButton } from "../auth/StartWorkspaceButton";
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
        <AuthLoading />
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
        <Route
          path="workspaces/:workspaceId/tokens"
          element={<TokenRoutePlaceholder apiClient={apiClient} />}
        />
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
    return <WorkspaceListSkeleton />;
  }

  if (workspaces.isError) {
    return <WorkspaceError error={workspaces.error} />;
  }

  if (!workspaces.data?.length) {
    return <Navigate replace to="/app/onboarding" />;
  }

  return <Navigate replace to={workspaceTokenPath(workspaces.data[0])} />;
}

function WorkspaceOnboarding({ apiClient }: WorkspaceRouteProps) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const workspaces = useWorkspaces(apiClient);

  const create = useMutation({
    mutationFn: () => createWorkspace(apiClient, { name: name.trim() }),
    onSuccess: async (workspace) => {
      await queryClient.invalidateQueries({ queryKey: ["workspaces"] });
      navigate(workspaceTokenPath(workspace));
    },
  });

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    create.mutate();
  }

  if (workspaces.isLoading) {
    return <WorkspaceListSkeleton />;
  }

  if (workspaces.isError) {
    return <WorkspaceError error={workspaces.error} />;
  }

  if (workspaces.data?.length) {
    return <Navigate replace to={workspaceTokenPath(workspaces.data[0])} />;
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

function TokenRoutePlaceholder({ apiClient }: WorkspaceRouteProps) {
  const { workspaceId } = useParams();
  const workspaces = useWorkspaces(apiClient);

  if (workspaces.isLoading) {
    return <TokenSetupSkeleton />;
  }

  if (workspaces.isError) {
    return <WorkspaceError error={workspaces.error} />;
  }

  const workspace = workspaces.data?.find((candidate) => candidate.id === workspaceId);

  if (!workspace) {
    return (
      <StatusPanel title="Workspace not found" tone="preview">
        <p>You do not have access to this workspace.</p>
      </StatusPanel>
    );
  }

  return (
    <section className="page-heading" aria-labelledby="tokens-title">
      <p className="eyebrow">Token setup</p>
      <h1 id="tokens-title">Token creation is next.</h1>
      <p>{workspace.name} is ready for token setup.</p>
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

function WorkspaceListSkeleton() {
  return (
    <>
      <section className="page-heading onboarding-heading" aria-label="Loading workspaces">
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

function TokenSetupSkeleton() {
  return (
    <section className="page-heading" aria-label="Loading token setup">
      <Skeleton className="skeleton-eyebrow" />
      <Skeleton className="skeleton-heading skeleton-heading-short" />
      <Skeleton className="skeleton-copy" />
    </section>
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

function workspaceTokenPath(workspace: Pick<Workspace, "id">) {
  return `/app/workspaces/${workspace.id}/tokens`;
}
