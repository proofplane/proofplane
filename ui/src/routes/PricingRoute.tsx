import { Link } from "react-router-dom";
import { Shell } from "../components/Shell";

export function PricingRoute() {
  return (
    <Shell>
      <section className="page-heading" aria-labelledby="pricing-title">
        <p className="eyebrow">Pricing</p>
        <h1 id="pricing-title">Self-serve first, sales later.</h1>
        <p>
          Proofplane starts with workspace setup so teams can inspect the scoped
          token model and evidence APIs before a sales conversation.
        </p>
        <Link className="button button-primary" to="/">
          Start from the public page
        </Link>
      </section>
    </Shell>
  );
}
