import type { ApiClient } from "./client";

export const tokenPermissions = [
  "read_evidence_requests",
  "write_evidence_requests",
  "read_evidence_submissions",
  "write_evidence_submissions",
  "read_controls",
  "write_controls",
] as const;

export type TokenPermission = (typeof tokenPermissions)[number];

export type PermissionPresetId =
  | "read-compliance-data"
  | "submit-evidence"
  | "manage-mappings"
  | "all-permissions"
  | "custom";

export type PermissionPreset = {
  id: PermissionPresetId;
  name: string;
  description: string;
  permissions: TokenPermission[];
};

export const permissionPresets: PermissionPreset[] = [
  {
    id: "read-compliance-data",
    name: "Read compliance data",
    description: "Inspect evidence requests, evidence submissions, and controls.",
    permissions: [
      "read_evidence_requests",
      "read_evidence_submissions",
      "read_controls",
    ],
  },
  {
    id: "submit-evidence",
    name: "Submit evidence",
    description: "Read evidence requests and create evidence submissions.",
    permissions: ["read_evidence_requests", "write_evidence_submissions"],
  },
  {
    id: "manage-mappings",
    name: "Manage mappings",
    description: "Read and update controls plus evidence request mappings.",
    permissions: [
      "read_controls",
      "write_controls",
      "read_evidence_requests",
      "write_evidence_requests",
    ],
  },
  {
    id: "all-permissions",
    name: "All permissions",
    description: "Grant every current workspace data-plane permission.",
    permissions: [...tokenPermissions],
  },
  {
    id: "custom",
    name: "Custom",
    description: "Choose exact granular permissions.",
    permissions: [],
  },
];

export type CreateApiTokenInput = {
  name: string;
  expires_at: string;
  permissions: TokenPermission[];
};

export type ApiToken = {
  id: string;
  name: string;
  workspace_id: string;
  permissions: TokenPermission[];
  expires_at: string;
  revoked_at: string | null;
  last_used_at: string | null;
  created_at: string;
};

export type IssuedApiToken = ApiToken & {
  api_token: string;
};

export function createApiToken(
  client: ApiClient,
  workspaceId: string,
  input: CreateApiTokenInput,
): Promise<IssuedApiToken> {
  return client.request<IssuedApiToken>(`/workspaces/${workspaceId}/api-tokens`, {
    body: JSON.stringify(input),
    headers: { "Content-Type": "application/json" },
    method: "POST",
  });
}

export function listApiTokens(
  client: ApiClient,
  workspaceId: string,
): Promise<ApiToken[]> {
  return client.request<ApiToken[]>(`/workspaces/${workspaceId}/api-tokens`);
}

export function revokeApiToken(
  client: ApiClient,
  workspaceId: string,
  tokenId: string,
): Promise<void> {
  return client.request<void>(`/workspaces/${workspaceId}/api-tokens/${tokenId}`, {
    method: "DELETE",
  });
}

export function permissionsForPreset(id: PermissionPresetId): TokenPermission[] {
  return [...(permissionPresets.find((preset) => preset.id === id)?.permissions ?? [])];
}
