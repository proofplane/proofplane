export type ApiClientOptions = {
  baseUrl?: string;
  getAccessToken?: () => Promise<string | undefined>;
};

export class ApiError extends Error {
  readonly status: number;
  readonly details: unknown;

  constructor(status: number, message: string, details?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.details = details;
  }
}

export type ApiClient = {
  request<T>(path: string, init?: RequestInit): Promise<T>;
};

export function createApiClient(options: ApiClientOptions = {}): ApiClient {
  const baseUrl =
    options.baseUrl ??
    import.meta.env.VITE_PROOFPLANE_API_BASE_URL ??
    "http://127.0.0.1:3000";

  return {
    async request<T>(path: string, init: RequestInit = {}) {
      const token = await options.getAccessToken?.();
      const headers = new Headers(init.headers);

      if (!headers.has("Accept")) {
        headers.set("Accept", "application/json");
      }

      if (token) {
        headers.set("Authorization", `Bearer ${token}`);
      }

      const response = await fetch(new URL(path, baseUrl), {
        ...init,
        headers,
      });

      const text = await response.text();
      const body = text ? parseJson(text) : undefined;

      if (!response.ok) {
        throw new ApiError(
          response.status,
          errorMessage(body) ?? response.statusText,
          body,
        );
      }

      return body as T;
    },
  };
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function errorMessage(body: unknown): string | undefined {
  if (body && typeof body === "object" && "message" in body) {
    const message = (body as { message?: unknown }).message;
    return typeof message === "string" ? message : undefined;
  }

  return undefined;
}
