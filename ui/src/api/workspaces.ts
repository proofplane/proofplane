import type { ApiClient } from "./client";

export type Workspace = {
  id: string;
  slug: string | null;
  name: string;
  role: string;
  created_at: string;
};

export type CreateWorkspaceInput = {
  name: string;
};

export function listWorkspaces(client: ApiClient): Promise<Workspace[]> {
  return client.request<Workspace[]>("/workspaces");
}

export function createWorkspace(
  client: ApiClient,
  input: CreateWorkspaceInput,
): Promise<Workspace> {
  return client.request<Workspace>("/workspaces", {
    body: JSON.stringify(input),
    headers: { "Content-Type": "application/json" },
    method: "POST",
  });
}
