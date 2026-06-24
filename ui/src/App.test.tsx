import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { App } from "./App";

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

it("renders the long-scroll homepage", () => {
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
      name: /Simplify the work, keep the audit trail/i,
    }),
  ).toBeInTheDocument();
  expect(container.textContent).not.toMatch(
    /without starting from an empty dashboard/i,
  );
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
