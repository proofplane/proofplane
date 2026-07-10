import { expect, it } from "vitest";
import { permissionsForPreset, tokenPermissions } from "./tokens";

it("maps token job presets to exact backend permissions", () => {
  expect(tokenPermissions).toEqual([
    "read_evidence_requests",
    "write_evidence_requests",
    "read_evidence_submissions",
    "write_evidence_submissions",
    "read_controls",
    "write_controls",
    "manage_auditor_access",
  ]);
  expect(permissionsForPreset("read-compliance-data")).toEqual([
    "read_evidence_requests",
    "read_evidence_submissions",
    "read_controls",
  ]);
  expect(permissionsForPreset("submit-evidence")).toEqual([
    "read_evidence_requests",
    "write_evidence_submissions",
  ]);
  expect(permissionsForPreset("manage-mappings")).toEqual([
    "read_controls",
    "write_controls",
    "read_evidence_requests",
    "write_evidence_requests",
  ]);
  expect(permissionsForPreset("all-permissions")).toEqual([...tokenPermissions]);
  expect(permissionsForPreset("custom")).toEqual([]);
});
