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
    page.getByRole("button", { name: /Log in or sign up/i }).first(),
  ).toBeVisible();
  await expect(page.getByText(/Book a Demo/i)).toHaveCount(0);
  const headerNav = page.getByRole("navigation", { name: "Primary navigation" });
  await expect(headerNav.getByRole("button", { name: /Log in or sign up/i })).toBeVisible();
  await expect(headerNav.getByRole("link", { name: "Home", exact: true })).toHaveCount(0);
  await expect(headerNav.getByRole("link", { name: "Pricing" })).toHaveCount(0);
  await expect(headerNav.getByRole("link", { name: "Docs" })).toHaveCount(0);
});

test("scrolling reveals the final setup card", async ({ page }) => {
  await page.goto("/");

  await page
    .getByRole("heading", { name: /Packet and MCP views are placeholders/i })
    .scrollIntoViewIfNeeded();

  await expect(
    page.getByRole("heading", {
      name: /Packet and MCP views are placeholders/i,
    }),
  ).toBeVisible();
  await expect(page.getByText("Auditor packet preview", { exact: true })).toBeVisible();
  await expect(page.getByText("Placeholder").first()).toBeVisible();
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
