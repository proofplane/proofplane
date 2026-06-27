import { Auth0Provider } from "@auth0/auth0-react";
import type { ReactNode } from "react";
import { getAuthConfig } from "./config";

type ProofplaneAuthProviderProps = {
  children: ReactNode;
};

export function ProofplaneAuthProvider({ children }: ProofplaneAuthProviderProps) {
  const config = getAuthConfig();

  if (!config) {
    return <>{children}</>;
  }

  return (
    <Auth0Provider
      authorizationParams={{
        audience: config.audience,
        redirect_uri: `${window.location.origin}/auth/callback`,
      }}
      clientId={config.clientId}
      domain={config.domain}
      onRedirectCallback={(appState) => {
        window.history.replaceState({}, document.title, appState?.returnTo ?? "/app");
      }}
      // When Auth0 uses a custom domain, refresh should restore the session without persistent browser token storage.
      useRefreshTokens
    >
      {children}
    </Auth0Provider>
  );
}
