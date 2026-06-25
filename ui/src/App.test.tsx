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
}));

vi.mock("@auth0/auth0-react", () => ({
  Auth0Provider: ({ children }: { children: React.ReactNode }) => children,
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
    screen.getAllByRole("button", { name: /Start workspace setup/i }),
  ).toHaveLength(2);
  expect(screen.getByRole("link", { name: /Pricing/i })).toHaveAttribute(
    "href",
    "/pricing",
  );
  expect(container.textContent).not.toMatch(
    /Book a Demo|without starting from an empty dashboard/i,
  );
});

it("sends the primary CTA to Auth0 signup/login", () => {
  renderAt("/");

  fireEvent.click(screen.getAllByRole("button", { name: /Start workspace setup/i })[0]);

  expect(auth0State.loginWithRedirect).toHaveBeenCalledWith({
    appState: { returnTo: "/app" },
    authorizationParams: {
      audience: "https://api.proofplane.com",
      redirect_uri: "http://localhost:3000/auth/callback",
    },
  });
});

it("shows a recoverable CTA error when Auth0 is not configured", () => {
  import.meta.env.VITE_AUTH0_DOMAIN = "";
  import.meta.env.VITE_AUTH0_CLIENT_ID = "";
  renderAt("/");

  fireEvent.click(screen.getAllByRole("button", { name: /Start workspace setup/i })[0]);

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

it("creates a workspace and routes to token creation", async () => {
  auth0State.isAuthenticated = true;
  const fetchMock = mockFetchJson({
    id: "workspace-123",
    slug: null,
    name: "Acme",
    role: "owner",
    created_at: "2026-06-24T00:00:00Z",
  });

  renderAt("/app/onboarding");

  fireEvent.change(screen.getByLabelText(/Workspace name/i), {
    target: { value: "Acme" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Create workspace/i }));

  await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
  expect(fetchMock.mock.calls[0][0].toString()).toBe(
    "http://127.0.0.1:3000/workspaces",
  );
  expect(fetchMock.mock.calls[0][1]).toMatchObject({
    body: JSON.stringify({ name: "Acme" }),
    method: "POST",
  });
  expect(
    await screen.findByRole("heading", { name: /Token creation is next/i }),
  ).toBeInTheDocument();
  expect(screen.getByText(/workspace-123/i)).toBeInTheDocument();
});

it("preserves workspace input when creation fails", async () => {
  auth0State.isAuthenticated = true;
  mockFetchJson(
    {
      error: {
        code: "slug_taken",
        message: "a workspace with this slug already exists",
        details: [],
      },
    },
    { status: 409 },
  );

  renderAt("/app/onboarding");

  fireEvent.change(screen.getByLabelText(/Workspace name/i), {
    target: { value: "Acme" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Create workspace/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    /workspace with this slug already exists/i,
  );
  expect(screen.getByLabelText(/Workspace name/i)).toHaveValue("Acme");
});

it("lets authenticated users resume existing workspaces", async () => {
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

  renderAt("/app");

  expect(
    await screen.findByRole("heading", { name: /Resume a workspace/i }),
  ).toBeInTheDocument();
  expect(screen.getByText("Existing Workspace")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: /Resume setup/i })).toHaveAttribute(
    "href",
    "/app/workspaces/workspace-456/tokens",
  );
});

function mockFetchJson(body: unknown, init: ResponseInit = {}) {
  const fetchMock = vi.fn<typeof fetch>(async () => {
    return new Response(JSON.stringify(body), {
      headers: { "Content-Type": "application/json" },
      status: init.status ?? 200,
      statusText: init.statusText,
    });
  });

  vi.stubGlobal("fetch", fetchMock);

  return fetchMock;
}
