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
    page.getByRole("link", { name: /Start SOC 2 Sandbox/i }).first(),
  ).toBeVisible();
  await expect(page.getByText(/without starting from an empty dashboard/i)).toHaveCount(
    0,
  );
});

test("scrolling reveals the auditor packet section", async ({ page }) => {
  await page.goto("/");

  await page
    .getByRole("heading", { name: /Simplify the work, keep the audit trail/i })
    .scrollIntoViewIfNeeded();

  await expect(
    page.getByRole("heading", {
      name: /Simplify the work, keep the audit trail/i,
    }),
  ).toBeVisible();
  await expect(page.getByText("Evidence gap identified")).toBeVisible();
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
