import type { ApiClient } from "./client";

export type AgentConnection = {
  id: string;
  client_name: string;
  status: "authorized" | "active";
  authorized_at: string;
  last_used_at: string | null;
};

export type AgentConnectionsResponse = {
  mcp_url: string;
  connections: AgentConnection[];
};

export function listAgentConnections(client: ApiClient): Promise<AgentConnectionsResponse> {
  return client.request<AgentConnectionsResponse>("/agent-connections");
}

export function revokeAgentConnection(client: ApiClient, id: string): Promise<void> {
  return client.request<void>(`/agent-connections/${id}`, { method: "DELETE" });
}
