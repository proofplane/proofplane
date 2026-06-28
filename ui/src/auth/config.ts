export type AuthConfig = {
  audience: string;
  clientId: string;
  domain: string;
};

const DEFAULT_AUDIENCE = "https://api.proofplane.com";

export function getAuthConfig(env = import.meta.env): AuthConfig | undefined {
  const domain = env.VITE_AUTH0_DOMAIN?.trim();
  const clientId = env.VITE_AUTH0_CLIENT_ID?.trim();
  const audience = env.VITE_AUTH0_AUDIENCE?.trim() || DEFAULT_AUDIENCE;

  if (!domain || !clientId) {
    return undefined;
  }

  return { audience, clientId, domain };
}
