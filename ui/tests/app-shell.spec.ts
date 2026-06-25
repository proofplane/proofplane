import { expect, test } from "@playwright/test";

test("renders the app shell", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByText("Proofplane").first()).toBeVisible();
  await expect(
    page.getByRole("heading", {
      name: /Compliance tasks, reduced to the next action/i,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /Start SOC 2 Sandbox/i }).first(),
  ).toBeVisible();
  await expect(page.getByText(/Book a Demo/i)).toHaveCount(0);
  await expect(page.getByRole("link", { name: /Pricing philosophy/i })).toHaveAttribute(
    "href",
    "/pricing",
  );
  await expect(page.getByRole("link", { name: "Docs" })).toHaveAttribute(
    "href",
    "/docs",
  );
});

test("scrolling reveals the final setup card", async ({ page }) => {
  await page.goto("/");

  await page
    .getByRole("heading", { name: /See what an auditor still needs/i })
    .scrollIntoViewIfNeeded();

  await expect(
    page.getByRole("heading", {
      name: /See what an auditor still needs/i,
    }),
  ).toBeVisible();
  await expect(page.getByText("Missing latest evidence")).toBeVisible();
});

test("unknown routes show a recoverable not-found state", async ({ page }) => {
  await page.goto("/unknown-route");

  await expect(
    page.getByRole("heading", {
      name: /This page is not part of the workspace yet/i,
    }),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: /Back to Proofplane/i })).toHaveAttribute(
    "href",
    "/",
  );
});

test("callback errors are recoverable", async ({ page }) => {
  await page.goto("/auth/callback?error=access_denied&error_description=Access%20denied");

  await expect(
    page.getByRole("heading", {
      name: /Sign in did not finish|Auth0 is not configured/i,
    }),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: /Back to Proofplane/i })).toHaveAttribute(
    "href",
    "/",
  );
});
