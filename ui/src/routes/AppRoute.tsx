import { Shell } from "../components/Shell";
import { StatusPanel } from "../components/StatusPanel";

export function AppRoute() {
  return (
    <Shell>
      <section className="page-heading" aria-labelledby="app-title">
        <p className="eyebrow">App shell</p>
        <h1 id="app-title">Self-onboarding workspace</h1>
        <p>
          This scaffold is ready for Auth0, workspace creation, token setup, and
          MCP preview flows in the next tickets.
        </p>
      </section>

      <StatusPanel title="Next step">
        Ticket 002 connects the public explainer to Auth0. Ticket 003 starts the
        workspace onboarding flow.
      </StatusPanel>
    </Shell>
  );
}
