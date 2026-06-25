import { useAuth0 } from "@auth0/auth0-react";
import { ArrowRight } from "lucide-react";
import { Link, Navigate } from "react-router-dom";
import { Shell } from "../components/Shell";
import { getAuthConfig } from "../auth/config";

export function AuthCallbackRoute() {
  const callbackError = new URLSearchParams(window.location.search).get("error_description");

  if (callbackError) {
    return (
      <AuthState
        eyebrow="Auth callback"
        title="Sign in did not finish."
        body={callbackError}
        retry={false}
      />
    );
  }

  const config = getAuthConfig();

  if (!config) {
    return (
      <AuthState
        eyebrow="Auth setup"
        title="Auth0 is not configured."
        body="Add the Auth0 Vite environment variables, then start workspace setup again."
        retry={false}
      />
    );
  }

  return <ConfiguredAuthCallbackRoute config={config} />;
}

type ConfiguredAuthCallbackRouteProps = {
  config: NonNullable<ReturnType<typeof getAuthConfig>>;
};

function ConfiguredAuthCallbackRoute({ config }: ConfiguredAuthCallbackRouteProps) {
  const { error, isAuthenticated, isLoading, loginWithRedirect } = useAuth0();
  const returnTo = "/app";

  if (isLoading) {
    return (
      <AuthState
        eyebrow="Auth callback"
        title="Finishing sign in."
        body="Proofplane is checking the Auth0 response before opening the workspace flow."
        retry={false}
      />
    );
  }

  if (error) {
    return (
      <AuthState
        eyebrow="Auth callback"
        title="Sign in did not finish."
        body={error.message || "Auth0 returned an error. You can retry without losing the public page."}
        onRetry={() =>
          loginWithRedirect({
            appState: { returnTo },
            authorizationParams: {
              audience: config.audience,
              redirect_uri: `${window.location.origin}/auth/callback`,
            },
          })
        }
        retry
      />
    );
  }

  if (isAuthenticated) {
    return <Navigate replace to={returnTo} />;
  }

  return (
    <AuthState
      eyebrow="Auth callback"
      title="Start sign in again."
      body="Auth0 did not return an active session. Retry from the public page."
      retry={false}
    />
  );
}

type AuthStateProps = {
  body: string;
  eyebrow: string;
  onRetry?: () => void;
  retry: boolean;
  title: string;
};

function AuthState({ body, eyebrow, onRetry, retry, title }: AuthStateProps) {
  return (
    <Shell>
      <section className="page-heading auth-state" aria-labelledby="auth-state-title">
        <p className="eyebrow">{eyebrow}</p>
        <h1 id="auth-state-title">{title}</h1>
        <p>{body}</p>
        <div className="actions">
          {retry ? (
            <button className="button button-primary" onClick={onRetry} type="button">
              Retry Auth0
              <ArrowRight aria-hidden="true" size={16} />
            </button>
          ) : null}
          <Link className="button button-secondary" to="/">
            Back to Proofplane
          </Link>
        </div>
      </section>
    </Shell>
  );
}
