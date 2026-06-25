import { Link } from "react-router-dom";
import { Shell } from "../components/Shell";

export function DocsRoute() {
  return (
    <Shell>
      <section className="page-heading" aria-labelledby="docs-title">
        <p className="eyebrow">Docs</p>
        <h1 id="docs-title">Proofplane setup notes are coming into the UI.</h1>
        <p>
          The first self-serve path starts with workspace setup. API and MCP setup
          guidance appears inside onboarding as the next tickets land.
        </p>
        <Link className="button button-primary" to="/">
          Start from the public page
        </Link>
      </section>
    </Shell>
  );
}
