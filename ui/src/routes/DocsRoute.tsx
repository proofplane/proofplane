import { Link } from "react-router-dom";
import { Shell } from "../components/Shell";

export function DocsRoute() {
  return (
    <Shell>
      <section className="page-heading" aria-labelledby="docs-title">
        <p className="eyebrow">Docs</p>
        <h1 id="docs-title">Setup docs are not in the UI yet.</h1>
        <p>
          Workspace setup is live. API and MCP setup notes stay in the repo until
          those screens ship.
        </p>
        <Link className="button button-primary" to="/">
          Start from the public page
        </Link>
      </section>
    </Shell>
  );
}
