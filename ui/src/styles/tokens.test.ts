import { readFileSync } from "node:fs";

const tokensCss = readFileSync("src/styles/tokens.css", "utf8");

it("exposes seeded design tokens as CSS variables", () => {
  expect(tokensCss).toContain("--color-primary: #2f6f5e;");
  expect(tokensCss).toContain("--color-canvas: #f7f3ea;");
  expect(tokensCss).toContain("--radius-md: 6px;");
  expect(tokensCss).toContain("--font-size-body: 1rem;");
});
