import { useAuth0 } from "@auth0/auth0-react";
import { LogOut, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { Link, NavLink } from "react-router-dom";
import { getAuthConfig } from "../auth/config";
import { StartWorkspaceButton } from "../auth/StartWorkspaceButton";

type ShellProps = {
  children: ReactNode;
};

function navClass({ isActive }: { isActive: boolean }) {
  return `shell-nav-link${isActive ? " shell-nav-link-active" : ""}`;
}

export function Shell({ children }: ShellProps) {
  return (
    <div className="shell">
      <header className="shell-header">
        <Link className="brand" to="/" aria-label="Proofplane home">
          <ShieldCheck aria-hidden="true" size={22} strokeWidth={2} />
          <span>Proofplane</span>
        </Link>
        <ShellNav />
      </header>
      <main className="shell-main">{children}</main>
    </div>
  );
}

function ShellNav() {
  if (!getAuthConfig()) {
    return <PublicNav />;
  }

  return <AuthNav />;
}

function AuthNav() {
  const { isAuthenticated, isLoading, logout } = useAuth0();

  if (!isAuthenticated) {
    return <PublicNav />;
  }

  return (
    <nav className="shell-nav" aria-label="Primary navigation">
      <NavLink className={navClass} to="/app">
        Workspaces
      </NavLink>
      <NavLink className={navClass} to="/docs">
        Docs
      </NavLink>
      {isLoading ? null : (
        <button
          className="button button-secondary shell-logout"
          onClick={() =>
            logout({
              logoutParams: { returnTo: window.location.origin },
            })
          }
          type="button"
        >
          Log out
          <LogOut aria-hidden="true" size={16} />
        </button>
      )}
    </nav>
  );
}

function PublicNav() {
  return (
    <nav className="shell-nav" aria-label="Primary navigation">
      <StartWorkspaceButton className="button button-primary shell-nav-cta" />
    </nav>
  );
}
