import { useAuth0 } from "@auth0/auth0-react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Check, Copy, Trash2 } from "lucide-react";
import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import {
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
  useParams,
} from "react-router-dom";
import { ApiError, createApiClient } from "../api/client";
import {
  createApiToken,
  listApiTokens,
  permissionPresets,
  permissionsForPreset,
  revokeApiToken,
  tokenPermissions,
  type ApiToken,
  type IssuedApiToken,
  type PermissionPresetId,
  type TokenPermission,
} from "../api/tokens";
import { createWorkspace, listWorkspaces, type Workspace } from "../api/workspaces";
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
        <AuthLoading />
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
          element={<TokenCreationRoute apiClient={apiClient} />}
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

function TokenCreationRoute({ apiClient }: WorkspaceRouteProps) {
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

  return <TokenCreationForm apiClient={apiClient} workspace={workspace} />;
}

function TokenCreationForm({
  apiClient,
  workspace,
}: WorkspaceRouteProps & { workspace: Workspace }) {
  const [name, setName] = useState(`${workspace.name} setup token`);
  const [expiresOn, setExpiresOn] = useState(defaultExpirationDate);
  const [presetId, setPresetId] = useState<PermissionPresetId>("read-compliance-data");
  const [customPermissions, setCustomPermissions] = useState<TokenPermission[]>([]);
  const [validationError, setValidationError] = useState("");
  const [issuedToken, setIssuedToken] = useState<IssuedApiToken | null>(null);
  const [savedAcknowledgement, setSavedAcknowledgement] = useState(false);
  const [showRevoked, setShowRevoked] = useState(false);
  const queryClient = useQueryClient();
  const tokens = useApiTokens(apiClient, workspace.id);

  const selectedPermissions =
    presetId === "custom" ? customPermissions : permissionsForPreset(presetId);

  const create = useMutation({
    mutationFn: () =>
      createApiToken(apiClient, workspace.id, {
        expires_at: new Date(`${expiresOn}T00:00:00.000Z`).toISOString(),
        name: name.trim(),
        permissions: selectedPermissions,
      }),
    onError: () => {
      setIssuedToken(null);
      setSavedAcknowledgement(false);
    },
    onSuccess: async (token) => {
      await queryClient.invalidateQueries({ queryKey: apiTokensQueryKey(workspace.id) });
      setIssuedToken(token);
      setSavedAcknowledgement(false);
    },
  });

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setIssuedToken(null);

    if (!selectedPermissions.length) {
      setValidationError("Choose at least one permission before creating a token.");
      return;
    }

    setValidationError("");
    create.mutate();
  }

  function toggleCustomPermission(permission: TokenPermission) {
    setValidationError("");
    setCustomPermissions((current) =>
      current.includes(permission)
        ? current.filter((candidate) => candidate !== permission)
        : [...current, permission],
    );
  }

  if (issuedToken) {
    return (
      <TokenSuccessPanel
        acknowledged={savedAcknowledgement}
        onAcknowledge={setSavedAcknowledgement}
        onContinue={() => {
          setIssuedToken(null);
        }}
        token={issuedToken}
      />
    );
  }

  return (
    <>
      <section className="page-heading token-heading" aria-labelledby="tokens-title">
        <p className="eyebrow">Token setup</p>
        <h1 id="tokens-title">Issue a scoped API token.</h1>
        <p>
          {workspace.name} is ready. Pick the job this token should do and review
          the exact permissions before creating it.
        </p>
      </section>

      <TokenListPanel
        apiClient={apiClient}
        isError={tokens.isError}
        isLoading={tokens.isLoading}
        showRevoked={showRevoked}
        tokens={tokens.data ?? []}
        workspaceId={workspace.id}
        onShowRevokedChange={setShowRevoked}
      />

      <form className="token-form" onSubmit={submit}>
        <div className="token-form-grid">
          <label className="field">
            <span>Token name</span>
            <input
              autoComplete="off"
              name="name"
              onChange={(event) => setName(event.target.value)}
              required
              value={name}
            />
          </label>

          <label className="field">
            <span>Expires on</span>
            <input
              min={todayDate()}
              name="expires_on"
              onChange={(event) => setExpiresOn(event.target.value)}
              required
              type="date"
              value={expiresOn}
            />
          </label>
        </div>

        <fieldset className="permission-presets">
          <legend>Token job</legend>
          {permissionPresets.map((preset) => (
            <label className="permission-preset" key={preset.id}>
              <input
                checked={presetId === preset.id}
                name="permission_preset"
                onChange={() => {
                  setPresetId(preset.id);
                  setValidationError("");
                }}
                type="radio"
              />
              <span>
                <strong>{preset.name}</strong>
                <small>{preset.description}</small>
              </span>
            </label>
          ))}
        </fieldset>

        {presetId === "custom" ? (
          <fieldset className="permission-checklist">
            <legend>Granular permissions</legend>
            {tokenPermissions.map((permission) => (
              <label key={permission}>
                <input
                  checked={customPermissions.includes(permission)}
                  onChange={() => toggleCustomPermission(permission)}
                  type="checkbox"
                />
                <code>{permission}</code>
              </label>
            ))}
          </fieldset>
        ) : null}

        <div className="permission-preview" aria-label="Selected permissions">
          <span>Selected permissions</span>
          {selectedPermissions.length ? (
            <ul>
              {selectedPermissions.map((permission) => (
                <li key={permission}>
                  <code>{permission}</code>
                </li>
              ))}
            </ul>
          ) : (
            <p>No permissions selected.</p>
          )}
        </div>

        {validationError ? (
          <p className="form-error" role="alert">
            {validationError}
          </p>
        ) : null}
        {create.isError ? <FormError error={create.error} /> : null}

        <Button
          disabled={create.isPending || !name.trim() || !expiresOn}
          type="submit"
        >
          {create.isPending ? "Creating token" : "Create token"}
          <ArrowRight aria-hidden="true" size={16} />
        </Button>
      </form>
    </>
  );
}

type TokenListPanelProps = {
  apiClient: ReturnType<typeof createApiClient>;
  isError: boolean;
  isLoading: boolean;
  onShowRevokedChange: (showRevoked: boolean) => void;
  showRevoked: boolean;
  tokens: ApiToken[];
  workspaceId: string;
};

function TokenListPanel({
  apiClient,
  isError,
  isLoading,
  onShowRevokedChange,
  showRevoked,
  tokens,
  workspaceId,
}: TokenListPanelProps) {
  const queryClient = useQueryClient();
  const visibleTokens = showRevoked
    ? tokens
    : tokens.filter((token) => !token.revoked_at);
  const revoke = useMutation({
    mutationFn: (tokenId: string) => revokeApiToken(apiClient, workspaceId, tokenId),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: apiTokensQueryKey(workspaceId) }),
  });

  function deleteToken(token: ApiToken) {
    const confirmed = window.confirm(
      `Delete "${token.name}"? If deleted, any users of this token will lose access immediately.`,
    );

    if (confirmed) {
      revoke.mutate(token.id);
    }
  }

  return (
    <section className="token-list-panel" aria-labelledby="token-list-title">
      <div className="token-list-header">
        <div>
          <h2 id="token-list-title">Tokens</h2>
          <p>Active tokens are listed by name. Revoked tokens are hidden by default.</p>
        </div>
        <label className="token-revoked-toggle">
          <input
            checked={showRevoked}
            onChange={(event) => onShowRevokedChange(event.target.checked)}
            type="checkbox"
          />
          <span>Show revoked</span>
        </label>
      </div>

      {isLoading ? (
        <div className="workspace-list" aria-hidden="true">
          <SkeletonWorkspaceRow />
          <SkeletonWorkspaceRow />
        </div>
      ) : null}

      {isError ? (
        <p className="form-error" role="alert">
          Tokens did not load.
        </p>
      ) : null}

      {!isLoading && !isError && visibleTokens.length === 0 ? (
        <p className="token-empty">
          {showRevoked ? "No tokens yet." : "No active tokens yet."}
        </p>
      ) : null}

      {!isLoading && !isError && visibleTokens.length ? (
        <div className="token-list">
          {visibleTokens.map((token) => (
            <article className="token-row" key={token.id}>
              <div>
                <h3>{token.name}</h3>
                <p>
                  {token.revoked_at
                    ? `Revoked ${formatDate(token.revoked_at)}`
                    : `Expires ${formatDate(token.expires_at)}`}
                </p>
                {token.permissions.length ? (
                  <ul className="token-permission-list" aria-label={`${token.name} permissions`}>
                    {token.permissions.map((permission) => (
                      <li key={permission}>
                        <code>{permission}</code>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="token-permissions-empty">No permissions</p>
                )}
              </div>
              {token.revoked_at ? (
                <span className="token-status">Revoked</span>
              ) : (
                <Button
                  disabled={revoke.isPending}
                  onClick={() => deleteToken(token)}
                  type="button"
                  variant="secondary"
                >
                  Delete
                  <Trash2 aria-hidden="true" size={16} />
                </Button>
              )}
            </article>
          ))}
        </div>
      ) : null}
    </section>
  );
}

type TokenSuccessPanelProps = {
  acknowledged: boolean;
  onAcknowledge: (acknowledged: boolean) => void;
  onContinue: () => void;
  token: IssuedApiToken;
};

function TokenSuccessPanel({
  acknowledged,
  onAcknowledge,
  onContinue,
  token,
}: TokenSuccessPanelProps) {
  const envValue = `PROOFPLANE_API_TOKEN=${token.api_token}`;
  const mcpConfig = JSON.stringify(
    {
      proofplane: {
        env: { PROOFPLANE_API_TOKEN: token.api_token },
        transport: "stdio",
      },
    },
    null,
    2,
  );

  return (
    <section className="token-success" aria-labelledby="token-success-title">
      <p className="eyebrow">Token issued</p>
      <h1 id="token-success-title">Save this token now.</h1>
      <p>
        Proofplane shows the raw token once. It will not appear again after you
        leave this screen.
      </p>

      <CopyBlock label="Raw token" value={token.api_token} />
      <CopyBlock label="Environment variable" value={envValue} />
      <CopyBlock label="MCP config preview" value={mcpConfig} multiline />

      <label className="token-acknowledgement">
        <input
          checked={acknowledged}
          onChange={(event) => onAcknowledge(event.target.checked)}
          type="checkbox"
        />
        <span>I saved this token.</span>
      </label>

      <Button disabled={!acknowledged} onClick={onContinue} type="button">
        Continue
        <Check aria-hidden="true" size={16} />
      </Button>
    </section>
  );
}

function CopyBlock({
  label,
  multiline = false,
  value,
}: {
  label: string;
  multiline?: boolean;
  value: string;
}) {
  const [copied, setCopied] = useState(false);
  const resetCopied = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (resetCopied.current) {
        window.clearTimeout(resetCopied.current);
      }
    };
  }, []);

  async function copyValue() {
    await navigator.clipboard?.writeText(value);
    setCopied(true);

    if (resetCopied.current) {
      window.clearTimeout(resetCopied.current);
    }

    resetCopied.current = window.setTimeout(() => setCopied(false), 1600);
  }

  return (
    <div className="copy-block">
      <div className="copy-block-header">
        <span>{label}</span>
        <Button
          aria-label={`Copy ${label}`}
          className={`copy-button${copied ? " copy-button-copied" : ""}`}
          onClick={copyValue}
          type="button"
          variant="secondary"
        >
          {copied ? (
            <Check aria-hidden="true" size={16} />
          ) : (
            <Copy aria-hidden="true" size={16} />
          )}
          <span className="copy-button-label" aria-live="polite">
            <span
              aria-hidden={copied}
              className={copied ? "copy-button-label-hidden" : ""}
            >
              Copy
            </span>
            <span
              aria-hidden={!copied}
              className={copied ? "" : "copy-button-label-hidden"}
            >
              Copied
            </span>
          </span>
        </Button>
      </div>
      <code className={multiline ? "copy-block-code copy-block-code-multiline" : "copy-block-code"}>
        {value}
      </code>
    </div>
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

function useApiTokens(apiClient: ReturnType<typeof createApiClient>, workspaceId: string) {
  return useQuery({
    queryFn: () => listApiTokens(apiClient, workspaceId),
    queryKey: apiTokensQueryKey(workspaceId),
    retry: false,
  });
}

function apiTokensQueryKey(workspaceId: string) {
  return ["api-tokens", workspaceId];
}

function workspaceTokenPath(workspace: Pick<Workspace, "id">) {
  return `/app/workspaces/${workspace.id}/tokens`;
}

function todayDate() {
  return dateInputValue(0);
}

function defaultExpirationDate() {
  return dateInputValue(30);
}

function dateInputValue(daysFromNow: number) {
  const date = new Date();
  date.setUTCDate(date.getUTCDate() + daysFromNow);
  return date.toISOString().slice(0, 10);
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
    year: "numeric",
  }).format(new Date(value));
}
