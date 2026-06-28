import { Link } from "react-router-dom";
import { Shell } from "../components/Shell";

export function NotFoundRoute() {
  return (
    <Shell>
      <section className="not-found" aria-labelledby="not-found-title">
        <p className="eyebrow">Route not found</p>
        <h1 id="not-found-title">This page is not part of the workspace yet.</h1>
        <p>
          Return to the Proofplane entry point and continue from workspace setup.
        </p>
        <Link className="button button-secondary" to="/">
          Back to Proofplane
        </Link>
      </section>
    </Shell>
  );
}
