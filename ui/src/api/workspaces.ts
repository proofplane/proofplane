import { ApiError, type ApiClient } from "./client";

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

export async function getWorkspace(client: ApiClient): Promise<Workspace | null> {
  try {
    return await client.request<Workspace>("/workspace");
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) {
      return null;
    }

    throw error;
  }
}

export function createWorkspace(
  client: ApiClient,
  input: CreateWorkspaceInput,
): Promise<Workspace> {
  return client.request<Workspace>("/workspace", {
    body: JSON.stringify(input),
    headers: { "Content-Type": "application/json" },
    method: "POST",
  });
}
