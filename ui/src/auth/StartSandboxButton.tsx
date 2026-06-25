import { useAuth0 } from "@auth0/auth0-react";
import { ArrowRight } from "lucide-react";
import { useState } from "react";
import { getAuthConfig } from "./config";

type StartSandboxButtonProps = {
  className?: string;
};

export function StartSandboxButton({ className }: StartSandboxButtonProps) {
  const config = getAuthConfig();

  if (!config) {
    return <MissingAuthConfigButton className={className} />;
  }

  return <ConfiguredStartSandboxButton className={className} config={config} />;
}

type ConfiguredStartSandboxButtonProps = StartSandboxButtonProps & {
  config: NonNullable<ReturnType<typeof getAuthConfig>>;
};

function ConfiguredStartSandboxButton({
  className,
  config,
}: ConfiguredStartSandboxButtonProps) {
  const { isLoading, loginWithRedirect } = useAuth0();
  const [error, setError] = useState<string>();

  async function startSandbox() {
    setError(undefined);

    try {
      await loginWithRedirect({
        appState: { returnTo: "/app" },
        authorizationParams: {
          audience: config.audience,
          redirect_uri: `${window.location.origin}/auth/callback`,
        },
      });
    } catch {
      setError("Auth0 did not start. Check the configuration and try again.");
    }
  }

  return (
    <span className="auth-action">
      <button
        className={className ?? "button button-primary"}
        disabled={isLoading}
        onClick={startSandbox}
        type="button"
      >
        {isLoading ? "Preparing Auth0" : "Start SOC 2 Sandbox"}
        <ArrowRight aria-hidden="true" size={16} />
      </button>
      {error ? (
        <span className="auth-inline-error" role="alert">
          {error}
        </span>
      ) : null}
    </span>
  );
}

function MissingAuthConfigButton({ className }: StartSandboxButtonProps) {
  const [error, setError] = useState<string>();

  return (
    <span className="auth-action">
      <button
        className={className ?? "button button-primary"}
        onClick={() => setError("Auth0 is not configured for this environment.")}
        type="button"
      >
        Start SOC 2 Sandbox
        <ArrowRight aria-hidden="true" size={16} />
      </button>
      {error ? (
        <span className="auth-inline-error" role="alert">
          {error}
        </span>
      ) : null}
    </span>
  );
}
