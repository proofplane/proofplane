import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, expect, vi } from "vitest";
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

it("renders the public explainer with MCP onboarding", () => {
  renderAt("/");

  expect(screen.getByText("Proofplane")).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Compliance tasks, reduced to the next action/i,
    }),
  ).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: /Create the workspace/i })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: /Connect an MCP client/i })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: /Collect compliance evidence/i })).toBeInTheDocument();
  expect(screen.getByText("Requested permissions")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: /Log in or sign up/i })).toHaveLength(2);
  expect(screen.getAllByRole("link", { name: "Pricing" })).toHaveLength(1);
  expect(screen.getAllByRole("link", { name: "Docs" })).toHaveLength(1);
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
  buttons.forEach((button) => expect(button).toBeDisabled());
});

it("shows a recoverable CTA error when Auth0 is not configured", () => {
  import.meta.env.VITE_AUTH0_DOMAIN = "";
  import.meta.env.VITE_AUTH0_CLIENT_ID = "";
  renderAt("/");

  fireEvent.click(screen.getAllByRole("button", { name: /Log in or sign up/i })[0]);

  expect(screen.getByRole("alert")).toHaveTextContent(/Auth0 is not configured/i);
  expect(auth0State.loginWithRedirect).not.toHaveBeenCalled();
});

it("renders callback errors with retry and public page recovery", () => {
  auth0State.error = new Error("Access denied");
  renderAt("/auth/callback");

  expect(screen.getByRole("heading", { name: /Sign in did not finish/i })).toBeInTheDocument();
  expect(screen.getByText("Access denied")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: /Back to Proofplane/i })).toHaveAttribute("href", "/");

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
});

it("redirects unauthenticated app users to Auth0 on the same path", async () => {
  renderAt("/app/connect?source=refresh#oauth");

  expect(screen.getByRole("status")).toHaveTextContent(/Finishing sign in/i);

  await waitFor(() => {
    expect(auth0State.loginWithRedirect).toHaveBeenCalledWith({
      appState: {
        returnTo: "/app/connect?source=refresh#oauth",
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
  expect(screen.getByRole("link", { name: /Back to Proofplane/i })).toHaveAttribute("href", "/");
});

it("routes authenticated users without a workspace to onboarding", async () => {
  auth0State.isAuthenticated = true;
  mockWorkspaceFetch(null);

  renderAt("/app");

  expect(await screen.findByRole("heading", { name: /Create a workspace/i })).toBeInTheDocument();
  expect(screen.getByLabelText(/Workspace name/i)).toBeInTheDocument();
});

it("renders authenticated navigation and logs out through Auth0", async () => {
  auth0State.isAuthenticated = true;
  mockWorkspaceFetch(null);

  renderAt("/app");

  expect(await screen.findByRole("link", { name: "Workspace" })).toHaveAttribute("href", "/app");
  expect(screen.getByRole("link", { name: "Docs" })).toHaveAttribute("href", "/docs");

  fireEvent.click(screen.getByRole("button", { name: /Log out/i }));

  expect(auth0State.logout).toHaveBeenCalledWith({
    logoutParams: { returnTo: "http://localhost:3000" },
  });
});

it("uses skeleton rows while loading workspace state", () => {
  auth0State.isAuthenticated = true;
  mockFetchPending();
  const { container } = renderAt("/app");

  expect(container.querySelectorAll(".skeleton-row")).toHaveLength(2);
});

it("creates a workspace and routes to MCP connection setup", async () => {
  auth0State.isAuthenticated = true;
  const fetchMock = mockWorkspaceCreationFetch();
  renderAt("/app/onboarding");

  fireEvent.change(await screen.findByLabelText(/Workspace name/i), {
    target: { value: "Acme" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Create workspace/i }));

  await waitFor(() =>
    expect(fetchMock.mock.calls.some((call) => call[1]?.method === "POST")).toBe(true),
  );
  const createCall = fetchMock.mock.calls.find((call) => call[1]?.method === "POST");
  expect(createCall?.[0].toString()).toBe("http://127.0.0.1:3000/workspace");
  expect(createCall?.[1]).toMatchObject({
    body: JSON.stringify({ name: "Acme" }),
    method: "POST",
  });
  expect(await screen.findByRole("heading", { name: /Connect an MCP client/i })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Acme" })).toBeInTheDocument();
});

it("preserves workspace input when creation fails", async () => {
  auth0State.isAuthenticated = true;
  mockFetchSequence([
    { error: { code: "not_found", message: "route not found", details: [] } },
    {
      error: {
        code: "user_already_has_workspace",
        message: "the user already belongs to a workspace",
        details: [],
      },
    },
  ], [404, 409]);

  renderAt("/app/onboarding");

  fireEvent.change(await screen.findByLabelText(/Workspace name/i), {
    target: { value: "Acme" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Create workspace/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent(/already belongs to a workspace/i);
  expect(screen.getByLabelText(/Workspace name/i)).toHaveValue("Acme");
});

it("routes authenticated users with a workspace to MCP connection setup", async () => {
  auth0State.isAuthenticated = true;
  mockWorkspaceFetch(workspaceResponse("Existing Workspace"));

  renderAt("/app");

  expect(await screen.findByRole("heading", { name: /Connect an MCP client/i })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Existing Workspace" })).toBeInTheDocument();
});

it("keeps authenticated users with a workspace out of onboarding", async () => {
  auth0State.isAuthenticated = true;
  mockWorkspaceFetch(workspaceResponse("Existing Workspace"));

  renderAt("/app/onboarding");

  expect(await screen.findByRole("heading", { name: /Connect an MCP client/i })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Existing Workspace" })).toBeInTheDocument();
});

function workspaceResponse(name: string) {
  return {
    id: "workspace-456",
    slug: null,
    name,
    role: "owner",
    created_at: "2026-06-24T00:00:00Z",
  };
}

function mockWorkspaceFetch(workspace: ReturnType<typeof workspaceResponse> | null) {
  const fetchMock = vi.fn<typeof fetch>(async () => {
    if (!workspace) {
      return jsonResponse({
        error: { code: "not_found", message: "route not found", details: [] },
      }, 404);
    }

    return jsonResponse(workspace);
  });

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}

function mockWorkspaceCreationFetch() {
  let created = false;
  const workspace = workspaceResponse("Acme");
  const fetchMock = vi.fn<typeof fetch>(async (_input, init) => {
    const method = init?.method ?? "GET";

    if (method === "POST") {
      created = true;
      return jsonResponse(workspace);
    }

    if (!created) {
      return jsonResponse({
        error: { code: "not_found", message: "route not found", details: [] },
      }, 404);
    }

    return jsonResponse(workspace);
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
