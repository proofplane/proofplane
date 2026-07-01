import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, expect, vi } from "vitest";
import type { ApiToken, IssuedApiToken } from "./api/tokens";
import { App } from "./App";

const auth0State = vi.hoisted(() => ({
  appState: undefined as { returnTo?: string } | undefined,
  error: undefined as Error | undefined,
  isAuthenticated: false,
  isLoading: false,
  getAccessTokenSilently: vi.fn(async () => "access-token"),
  loginWithRedirect: vi.fn(),
  logout: vi.fn(),
}));
const auth0ProviderProps = vi.hoisted(() => [] as Record<string, unknown>[]);

vi.mock("@auth0/auth0-react", () => ({
  Auth0Provider: (props: { children: React.ReactNode }) => {
    auth0ProviderProps.push(props);
    return props.children;
  },
  useAuth0: () => auth0State,
}));

function renderAt(path: string) {
  return render(
    <MemoryRouter
      initialEntries={[path]}
      future={{ v7_relativeSplatPath: true, v7_startTransition: true }}
    >
      <App />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  auth0State.appState = undefined;
  auth0State.error = undefined;
  auth0State.isAuthenticated = false;
  auth0State.isLoading = false;
  auth0State.getAccessTokenSilently.mockReset();
  auth0State.getAccessTokenSilently.mockResolvedValue("access-token");
  auth0State.loginWithRedirect.mockReset();
  auth0State.logout.mockReset();
  auth0ProviderProps.length = 0;
  vi.unstubAllGlobals();
  import.meta.env.VITE_AUTH0_DOMAIN = "proofplane.auth0.com";
  import.meta.env.VITE_AUTH0_CLIENT_ID = "client_123";
  import.meta.env.VITE_AUTH0_AUDIENCE = "https://api.proofplane.com";
});

it("renders the public explainer without a demo gate", () => {
  const { container } = renderAt("/");

  expect(screen.getByText("Proofplane")).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Compliance tasks, reduced to the next action/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: /Create the workspace/i }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Issue a scoped API token/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Use the data APIs/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Packet and MCP views are placeholders/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /What works now, and what comes later/i,
    }),
  ).toBeInTheDocument();
  expect(screen.getByText("Token permissions")).toBeInTheDocument();
  expect(
    screen.getAllByRole("button", { name: /Log in or sign up/i }),
  ).toHaveLength(2);
  expect(screen.queryByRole("link", { name: "Home" })).not.toBeInTheDocument();
  expect(screen.getAllByRole("link", { name: "Pricing" })).toHaveLength(1);
  expect(screen.getAllByRole("link", { name: "Docs" })).toHaveLength(1);
  expect(screen.queryByRole("button", { name: /Log out/i })).not.toBeInTheDocument();
  expect(container.textContent).not.toMatch(
    /Book a Demo|without starting from an empty dashboard|workspace_id|request_id|\/workspaces\/\{id\}/i,
  );
});

it("configures Auth0 refresh tokens without persistent browser storage", () => {
  renderAt("/");

  expect(auth0ProviderProps[0]).toMatchObject({
    clientId: "client_123",
    domain: "proofplane.auth0.com",
    useRefreshTokens: true,
  });
  expect(auth0ProviderProps[0]).not.toHaveProperty("cacheLocation");
});

it("renders MCP consent without exposing OAuth credentials", async () => {
  auth0State.isAuthenticated = true;
  vi.stubGlobal("fetch", vi.fn(async (input: URL | RequestInfo) => {
    const path = input.toString();
    const body = path.includes("/oauth/requests/")
      ? { id: "request-1", client_name: "Proofplane Local", scopes: ["read_controls", "offline_access"], expires_at: "2030-01-01T00:00:00Z" }
      : [{ id: "workspace-1", slug: null, name: "SOC 2", role: "owner", created_at: "2026-01-01T00:00:00Z" }];
    return new Response(JSON.stringify(body), { status: 200 });
  }));

  renderAt("/connect/mcp/authorize?request_id=request-1");

  expect(await screen.findByRole("heading", { name: "Authorize Proofplane Local" })).toBeInTheDocument();
  expect(screen.getByText("Permissions: read_controls")).toBeInTheDocument();
  expect(document.body.textContent).not.toMatch(/v4\.public|access_token|refresh_token/);
});

it("sends the primary CTA to Auth0 signup/login", () => {
  renderAt("/");

  fireEvent.click(screen.getAllByRole("button", { name: /Log in or sign up/i })[0]);

  expect(auth0State.loginWithRedirect).toHaveBeenCalledWith({
    appState: { returnTo: "/app" },
    authorizationParams: {
      audience: "https://api.proofplane.com",
      redirect_uri: "http://localhost:3000/auth/callback",
    },
  });
});

it("keeps the CTA label stable while Auth0 initializes", () => {
  auth0State.isLoading = true;
  renderAt("/");

  const buttons = screen.getAllByRole("button", { name: /Log in or sign up/i });

  expect(buttons).toHaveLength(2);
  expect(screen.queryByText(/Preparing Auth0/i)).not.toBeInTheDocument();
  buttons.forEach((button) => expect(button).toBeDisabled());
});

it("shows a recoverable CTA error when Auth0 is not configured", () => {
  import.meta.env.VITE_AUTH0_DOMAIN = "";
  import.meta.env.VITE_AUTH0_CLIENT_ID = "";
  renderAt("/");

  fireEvent.click(screen.getAllByRole("button", { name: /Log in or sign up/i })[0]);

  expect(screen.getByRole("alert")).toHaveTextContent(
    /Auth0 is not configured/i,
  );
  expect(auth0State.loginWithRedirect).not.toHaveBeenCalled();
});

it("renders callback errors with retry and public page recovery", () => {
  auth0State.error = new Error("Access denied");
  renderAt("/auth/callback");

  expect(
    screen.getByRole("heading", { name: /Sign in did not finish/i }),
  ).toBeInTheDocument();
  expect(screen.getByText("Access denied")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: /Back to Proofplane/i })).toHaveAttribute(
    "href",
    "/",
  );

  fireEvent.click(screen.getByRole("button", { name: /Retry Auth0/i }));

  expect(auth0State.loginWithRedirect).toHaveBeenCalledWith({
    appState: { returnTo: "/app" },
    authorizationParams: {
      audience: "https://api.proofplane.com",
      redirect_uri: "http://localhost:3000/auth/callback",
    },
  });
});

it("uses a centered spinner while Auth0 is finishing sign in", () => {
  auth0State.isLoading = true;
  renderAt("/auth/callback");

  expect(screen.getByRole("status")).toHaveTextContent(/Finishing sign in/i);
  expect(
    screen.queryByRole("heading", { name: /Finishing sign in/i }),
  ).not.toBeInTheDocument();
});

it("uses a centered spinner while opening the app session", () => {
  auth0State.isLoading = true;
  renderAt("/app");

  expect(screen.getByRole("status")).toHaveTextContent(/Finishing sign in/i);
  expect(screen.queryByText(/Checking your Auth0 session/i)).not.toBeInTheDocument();
});

it("redirects unauthenticated app users to Auth0 on the same path", async () => {
  renderAt("/app/workspaces/workspace-789/tokens?source=refresh#token");

  expect(
    screen.queryByRole("heading", { name: /Sign in to resume setup/i }),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent(/Finishing sign in/i);
  expect(screen.queryByText(/Start with Auth0/i)).not.toBeInTheDocument();

  await waitFor(() => {
    expect(auth0State.loginWithRedirect).toHaveBeenCalledWith({
      appState: {
        returnTo: "/app/workspaces/workspace-789/tokens?source=refresh#token",
      },
      authorizationParams: {
        audience: "https://api.proofplane.com",
        redirect_uri: "http://localhost:3000/auth/callback",
      },
    });
  });
});

it("renders a recoverable not-found route", () => {
  renderAt("/missing");

  expect(
    screen.getByRole("heading", {
      name: /This page is not part of the workspace yet/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("link", { name: /Back to Proofplane/i }),
  ).toHaveAttribute("href", "/");
});

it("routes authenticated users without workspaces to onboarding", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson([]);

  renderAt("/app");

  expect(
    await screen.findByRole("heading", { name: /Create a workspace/i }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText(/Workspace name/i)).toBeInTheDocument();
});

it("renders authenticated navigation and logs out through Auth0", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson([]);

  renderAt("/app");

  expect(await screen.findByRole("link", { name: "Workspace" })).toHaveAttribute(
    "href",
    "/app",
  );
  expect(screen.queryByRole("link", { name: "New workspace" })).not.toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Docs" })).toHaveAttribute("href", "/docs");

  fireEvent.click(screen.getByRole("button", { name: /Log out/i }));

  expect(auth0State.logout).toHaveBeenCalledWith({
    logoutParams: { returnTo: "http://localhost:3000" },
  });
});

it("uses skeleton rows while loading workspaces", () => {
  auth0State.isAuthenticated = true;
  mockFetchPending();
  const { container } = renderAt("/app");

  expect(container.querySelectorAll(".skeleton-row")).toHaveLength(2);
  expect(screen.queryByText(/Checking for existing workspaces/i)).not.toBeInTheDocument();
});

it("creates a workspace and routes to token creation", async () => {
  auth0State.isAuthenticated = true;
  const fetchMock = mockWorkspaceCreationFetch();
  const { container } = renderAt("/app/onboarding");

  fireEvent.change(await screen.findByLabelText(/Workspace name/i), {
    target: { value: "Acme" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Create workspace/i }));

  await waitFor(() =>
    expect(fetchMock.mock.calls.some((call) => call[1]?.method === "POST")).toBe(true),
  );
  const createCall = fetchMock.mock.calls.find((call) => call[1]?.method === "POST");
  expect(createCall?.[0].toString()).toBe(
    "http://127.0.0.1:3000/workspaces",
  );
  expect(createCall?.[1]).toMatchObject({
    body: JSON.stringify({ name: "Acme" }),
    method: "POST",
  });
  expect(
    await screen.findByRole("heading", { name: /Issue a scoped API token/i }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText(/Token name/i)).toHaveValue("Acme setup token");
  expect(container.textContent).not.toMatch(/workspace-123/i);
});

it("preserves workspace input when creation fails", async () => {
  auth0State.isAuthenticated = true;
  mockFetchSequence([
    [],
    {
      error: {
        code: "slug_taken",
        message: "a workspace with this slug already exists",
        details: [],
      },
    },
  ], [200, 409]);

  renderAt("/app/onboarding");

  fireEvent.change(await screen.findByLabelText(/Workspace name/i), {
    target: { value: "Acme" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Create workspace/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    /workspace with this slug already exists/i,
  );
  expect(screen.getByLabelText(/Workspace name/i)).toHaveValue("Acme");
});

it("routes authenticated users with a workspace to token setup", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson([
    {
      id: "workspace-456",
      slug: null,
      name: "Existing Workspace",
      role: "owner",
      created_at: "2026-06-24T00:00:00Z",
    },
  ]);

  const { container } = renderAt("/app");

  expect(
    await screen.findByRole("heading", { name: /Issue a scoped API token/i }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText(/Token name/i)).toHaveValue(
    "Existing Workspace setup token",
  );
  expect(container.textContent).not.toMatch(/workspace-456/i);
  expect(screen.queryByRole("link", { name: /Resume setup/i })).not.toBeInTheDocument();
  expect(screen.queryByText(/Create another workspace/i)).not.toBeInTheDocument();
});

it("keeps authenticated users with a workspace out of onboarding", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson([
    {
      id: "workspace-456",
      slug: null,
      name: "Existing Workspace",
      role: "owner",
      created_at: "2026-06-24T00:00:00Z",
    },
  ]);

  renderAt("/app/onboarding");

  expect(
    await screen.findByRole("heading", { name: /Issue a scoped API token/i }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText(/Token name/i)).toHaveValue(
    "Existing Workspace setup token",
  );
  expect(screen.queryByLabelText(/Workspace name/i)).not.toBeInTheDocument();
});

it("shows the workspace name on token setup routes", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson([
    {
      id: "workspace-789",
      slug: null,
      name: "Named Workspace",
      role: "owner",
      created_at: "2026-06-24T00:00:00Z",
    },
  ]);

  const { container } = renderAt("/app/workspaces/workspace-789/tokens");

  expect(
    await screen.findByRole("heading", { name: /Issue a scoped API token/i }),
  ).toBeInTheDocument();
  expect(screen.getByLabelText(/Token name/i)).toHaveValue(
    "Named Workspace setup token",
  );
  expect(container.textContent).not.toMatch(/workspace-789/i);
});

it("shows exact granular permissions before token creation", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson([
    {
      id: "workspace-789",
      slug: null,
      name: "Named Workspace",
      role: "owner",
      created_at: "2026-06-24T00:00:00Z",
    },
  ]);

  renderAt("/app/workspaces/workspace-789/tokens");

  expect(await screen.findByLabelText("Selected permissions")).toHaveTextContent(
    "read_evidence_requests",
  );
  expect(screen.getByLabelText("Selected permissions")).toHaveTextContent(
    "read_evidence_submissions",
  );
  expect(screen.getByLabelText("Selected permissions")).toHaveTextContent(
    "read_controls",
  );

  fireEvent.click(screen.getByLabelText(/All permissions/i));

  expect(screen.getByLabelText("Selected permissions")).toHaveTextContent(
    "write_evidence_requests",
  );
  expect(screen.getByLabelText("Selected permissions")).toHaveTextContent(
    "write_evidence_submissions",
  );
  expect(screen.getByLabelText("Selected permissions")).toHaveTextContent(
    "write_controls",
  );
});

it("blocks custom token creation without selected permissions", async () => {
  auth0State.isAuthenticated = true;
  const fetchMock = mockFetchJson([
    {
      id: "workspace-789",
      slug: null,
      name: "Named Workspace",
      role: "owner",
      created_at: "2026-06-24T00:00:00Z",
    },
  ]);

  renderAt("/app/workspaces/workspace-789/tokens");

  fireEvent.click(await screen.findByLabelText(/Custom/i));
  fireEvent.click(screen.getByRole("button", { name: /Create token/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    /Choose at least one permission/i,
  );
  expect(fetchMock).toHaveBeenCalledTimes(2);
});

it("creates a token and shows the raw token once", async () => {
  auth0State.isAuthenticated = true;
  const fetchMock = mockTokenRouteFetch({
    postResponse: issuedTokenResponse(),
    tokens: [],
  });

  renderAt("/app/workspaces/workspace-789/tokens");

  fireEvent.click(await screen.findByRole("button", { name: /Create token/i }));

  await waitFor(() =>
    expect(fetchMock.mock.calls.some((call) => call[1]?.method === "POST")).toBe(true),
  );
  const createCall = fetchMock.mock.calls.find((call) => call[1]?.method === "POST");
  expect(createCall?.[0].toString()).toBe(
    "http://127.0.0.1:3000/workspaces/workspace-789/api-tokens",
  );
  expect(JSON.parse(createCall?.[1]?.body as string)).toMatchObject({
    name: "Named Workspace setup token",
    permissions: [
      "read_evidence_requests",
      "read_evidence_submissions",
      "read_controls",
    ],
  });
  expect(await screen.findByText("ppat_test_raw_token")).toBeInTheDocument();
  expect(screen.getByText(/PROOFPLANE_API_TOKEN=ppat_test_raw_token/i)).toBeInTheDocument();

  fireEvent.click(screen.getByLabelText(/I saved this token/i));
  fireEvent.click(screen.getByRole("button", { name: /Continue/i }));

  expect(screen.queryByText("ppat_test_raw_token")).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Tokens" })).toBeInTheDocument();
});

it("does not retain raw token state when token creation fails", async () => {
  auth0State.isAuthenticated = true;
  mockTokenRouteFetch({
    postResponse: {
      error: {
        message: "invalid permissions",
      },
    },
    postStatus: 400,
    tokens: [],
  });

  renderAt("/app/workspaces/workspace-789/tokens");

  fireEvent.click(await screen.findByRole("button", { name: /Create token/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent(/invalid permissions/i);
  expect(screen.queryByText(/ppat_/i)).not.toBeInTheDocument();
});

it("lists active tokens by name on refresh", async () => {
  auth0State.isAuthenticated = true;
  mockTokenRouteFetch({
    tokens: [listedToken({ id: "token-active", name: "CI token" })],
  });

  renderAt("/app/workspaces/workspace-789/tokens");

  expect(await screen.findByRole("heading", { name: "Tokens" })).toBeInTheDocument();
  expect(await screen.findByRole("heading", { name: "CI token" })).toBeInTheDocument();
  expect(screen.getByText("Expires Jul 24, 2026")).toBeInTheDocument();
  expect(screen.getByLabelText("CI token permissions")).toHaveTextContent(
    "read_evidence_requests",
  );
  expect(screen.getByLabelText("CI token permissions")).toHaveTextContent(
    "read_evidence_submissions",
  );
  expect(screen.getByLabelText("CI token permissions")).toHaveTextContent(
    "read_controls",
  );
  expect(screen.getByRole("button", { name: /Delete/i })).toBeInTheDocument();
});

it("confirms before deleting a token", async () => {
  auth0State.isAuthenticated = true;
  const confirm = vi.fn(() => false);
  vi.stubGlobal("confirm", confirm);
  const fetchMock = mockTokenRouteFetch({
    tokens: [listedToken({ id: "token-active", name: "CI token" })],
  });

  renderAt("/app/workspaces/workspace-789/tokens");

  fireEvent.click(await screen.findByRole("button", { name: /Delete/i }));

  expect(confirm).toHaveBeenCalledWith(
    expect.stringContaining("any users of this token will lose access immediately"),
  );
  expect(fetchMock.mock.calls.some((call) => call[1]?.method === "DELETE")).toBe(false);
});

it("deletes active tokens and hides revoked tokens until toggled", async () => {
  auth0State.isAuthenticated = true;
  vi.stubGlobal("confirm", vi.fn(() => true));
  const fetchMock = mockTokenRouteFetch({
    tokens: [
      listedToken({ id: "token-active", name: "CI token" }),
      listedToken({
        id: "token-revoked",
        name: "Old token",
        revoked_at: "2026-06-25T00:00:00Z",
      }),
    ],
    tokensAfterDelete: [
      listedToken({
        id: "token-active",
        name: "CI token",
        revoked_at: "2026-06-26T00:00:00Z",
      }),
      listedToken({
        id: "token-revoked",
        name: "Old token",
        revoked_at: "2026-06-25T00:00:00Z",
      }),
    ],
  });

  renderAt("/app/workspaces/workspace-789/tokens");

  fireEvent.click(await screen.findByRole("button", { name: /Delete/i }));

  await waitFor(() =>
    expect(fetchMock.mock.calls.some((call) => call[1]?.method === "DELETE")).toBe(true),
  );
  await waitFor(() =>
    expect(screen.getByText("No active tokens yet.")).toBeInTheDocument(),
  );
  expect(screen.queryByRole("heading", { name: "CI token" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByLabelText(/Show revoked/i));

  expect(await screen.findByRole("heading", { name: "CI token" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Old token" })).toBeInTheDocument();
  expect(screen.getByLabelText("CI token permissions")).toHaveTextContent(
    "read_controls",
  );
  expect(screen.getByLabelText("Old token permissions")).toHaveTextContent(
    "read_controls",
  );
  expect(screen.getAllByText("Revoked")).toHaveLength(2);
});

it("uses skeleton content while loading token setup", () => {
  auth0State.isAuthenticated = true;
  mockFetchPending();
  const { container } = renderAt("/app/workspaces/workspace-789/tokens");

  expect(container.querySelector(".skeleton-heading-short")).toBeInTheDocument();
  expect(screen.queryByText(/Checking workspace access/i)).not.toBeInTheDocument();
});

function issuedTokenResponse(): IssuedApiToken {
  return {
    id: "token-123",
    name: "Named Workspace setup token",
    workspace_id: "workspace-789",
    permissions: [
      "read_evidence_requests",
      "read_evidence_submissions",
      "read_controls",
    ],
    expires_at: "2026-07-24T00:00:00Z",
    revoked_at: null,
    last_used_at: null,
    created_at: "2026-06-24T00:00:00Z",
    api_token: "ppat_test_raw_token",
  };
}

function listedToken(overrides: Partial<ApiToken> = {}): ApiToken {
  const { api_token, ...token } = issuedTokenResponse();

  return {
    ...token,
    ...overrides,
  };
}

function mockFetchJson(body: unknown, init: ResponseInit = {}) {
  const fetchMock = vi.fn<typeof fetch>(async (input, requestInit) => {
    const url = input.toString();
    const method = requestInit?.method ?? "GET";
    const responseBody =
      method === "GET" && url.endsWith("/api-tokens") ? [] : body;

    return new Response(JSON.stringify(responseBody), {
      headers: { "Content-Type": "application/json" },
      status: init.status ?? 200,
      statusText: init.statusText,
    });
  });

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}

function mockTokenRouteFetch({
  postResponse,
  postStatus = 200,
  tokens,
  tokensAfterDelete,
}: {
  postResponse?: unknown;
  postStatus?: number;
  tokens: unknown[];
  tokensAfterDelete?: unknown[];
}) {
  let deleted = false;
  const workspace = [
    {
      id: "workspace-789",
      slug: null,
      name: "Named Workspace",
      role: "owner",
      created_at: "2026-06-24T00:00:00Z",
    },
  ];
  const fetchMock = vi.fn<typeof fetch>(async (input, init) => {
    const url = input.toString();
    const method = init?.method ?? "GET";

    if (method === "POST") {
      return jsonResponse(postResponse ?? issuedTokenResponse(), postStatus);
    }

    if (method === "DELETE") {
      deleted = true;
      return new Response(null, { status: 204 });
    }

    if (url.endsWith("/api-tokens")) {
      return jsonResponse(deleted ? (tokensAfterDelete ?? tokens) : tokens);
    }

    return jsonResponse(workspace);
  });

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}

function mockWorkspaceCreationFetch() {
  let created = false;
  const workspace = {
    id: "workspace-123",
    slug: null,
    name: "Acme",
    role: "owner",
    created_at: "2026-06-24T00:00:00Z",
  };
  const fetchMock = vi.fn<typeof fetch>(async (input, init) => {
    const url = input.toString();
    const method = init?.method ?? "GET";

    if (method === "POST") {
      created = true;
      return jsonResponse(workspace);
    }

    if (url.endsWith("/api-tokens")) {
      return jsonResponse([]);
    }

    return jsonResponse(created ? [workspace] : []);
  });

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { "Content-Type": "application/json" },
    status,
  });
}

function mockFetchPending() {
  const fetchMock = vi.fn<typeof fetch>(() => new Promise<Response>(() => {}));

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}

function mockFetchSequence(bodies: unknown[], statuses: number[] = []) {
  const fetchMock = vi.fn<typeof fetch>(async () => {
    const body = bodies.shift();
    const status = statuses.shift();

    return new Response(JSON.stringify(body), {
      headers: { "Content-Type": "application/json" },
      status: status ?? 200,
    });
  });

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}
