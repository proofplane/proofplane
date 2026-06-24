import { ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";

type ShellProps = {
  children: ReactNode;
};

export function Shell({ children }: ShellProps) {
  return (
    <div className="shell">
      <header className="shell-header">
        <Link className="brand" to="/" aria-label="Proofplane home">
          <ShieldCheck aria-hidden="true" size={22} strokeWidth={2} />
          <span>Proofplane</span>
        </Link>
      </header>
      <main className="shell-main">{children}</main>
    </div>
  );
}
