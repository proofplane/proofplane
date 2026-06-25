import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, expect, vi } from "vitest";
import { App } from "./App";

const auth0State = vi.hoisted(() => ({
  appState: undefined as { returnTo?: string } | undefined,
  error: undefined as Error | undefined,
  isAuthenticated: false,
  isLoading: false,
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
  auth0State.loginWithRedirect.mockReset();
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
      name: /Pick the job the token should do/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Let agents inspect the right records/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /See what an auditor still needs/i,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", {
      name: /Four steps from setup to packet clarity/i,
    }),
  ).toBeInTheDocument();
  expect(screen.getByText("Token preset")).toBeInTheDocument();
  expect(
    screen.getAllByRole("button", { name: /Start SOC 2 Sandbox/i }),
  ).toHaveLength(2);
  expect(screen.getByRole("link", { name: /Pricing philosophy/i })).toHaveAttribute(
    "href",
    "/pricing",
  );
  expect(container.textContent).not.toMatch(
    /Book a Demo|without starting from an empty dashboard/i,
  );
});

it("sends the primary CTA to Auth0 signup/login", () => {
  renderAt("/");

  fireEvent.click(screen.getAllByRole("button", { name: /Start SOC 2 Sandbox/i })[0]);

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

  fireEvent.click(screen.getAllByRole("button", { name: /Start SOC 2 Sandbox/i })[0]);

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
