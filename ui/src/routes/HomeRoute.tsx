import { Bot, FileCheck2, KeyRound, Layers3 } from "lucide-react";
import { Link } from "react-router-dom";
import { StartWorkspaceButton } from "../auth/StartWorkspaceButton";
import { Shell } from "../components/Shell";

const sections = [
  {
    eyebrow: "Step 01",
    title: "Create the workspace.",
    body: "Start with a tenant boundary and a place where compliance work has an owner.",
    icon: Layers3,
    artifact: "Workspace boundary",
    result: "Workspace created",
    summary: "Tenant boundary",
    visual: <WorkspaceStepVisual />,
  },
  {
    eyebrow: "Step 02",
    title: "Issue a scoped API token.",
    body: "Create a workspace-bound token with explicit permission strings. The backend can list and revoke those tokens later.",
    icon: KeyRound,
    artifact: "Scoped token",
    result: "Token endpoint ready",
    summary: "Token API",
    visual: <PermissionStepVisual />,
  },
  {
    eyebrow: "Step 03",
    title: "Use the data APIs.",
    body: "Controls, evidence requests, submissions, attachments, and mappings are backend routes today.",
    icon: Bot,
    artifact: "Compliance records",
    result: "REST APIs available",
    summary: "Data APIs",
    visual: <DataApiStepVisual />,
  },
  {
    eyebrow: "Step 04",
    title: "Packet and MCP views are placeholders.",
    body: "The backend does not yet provide production MCP tools or auditor packet previews. The UI marks both as not built yet.",
    icon: FileCheck2,
    artifact: "Later screens",
    result: "Not implemented yet",
    summary: "Later UI",
    visual: <PlaceholderStepVisual />,
  },
];

export function HomeRoute() {
  return (
    <Shell>
      <article className="landing-scroll">
        <section className="landing-hero scroll-section" aria-labelledby="home-title">
          <div className="section-copy">
            <p className="eyebrow">SOC 2 compliance infrastructure</p>
            <h1 id="home-title">Compliance tasks, reduced to the next action.</h1>
            <p className="lede">
              Proofplane currently supports workspace setup, scoped tokens, and
              backend APIs for controls, evidence requests, submissions, and attachments.
            </p>
            <div className="actions">
              <StartWorkspaceButton />
              <Link className="button button-secondary" to="/pricing">
                Pricing
              </Link>
              <Link className="button button-secondary" to="/docs">
                Docs
              </Link>
            </div>
          </div>
          <div className="sticky-visual" aria-hidden="true">
            <HeroVisual />
          </div>
        </section>

        <section className="step-card-flow" aria-labelledby="step-card-flow-title">
          <div className="step-card-heading">
            <div>
              <p className="eyebrow">How it works</p>
              <h2 id="step-card-flow-title">What works now, and what comes later.</h2>
            </div>
          </div>
          <div className="step-card-list">
            {sections.map(
              ({ artifact, body, eyebrow, icon: Icon, result, title, visual }, index) => {
                const step = eyebrow.replace("Step ", "");

                return (
                  <article
                    className="step-card"
                    data-step={step}
                    key={title}
                    style={{ zIndex: index + 1 }}
                  >
                    <div className="step-card-copy" data-step={step}>
                      <p className="eyebrow">{eyebrow}</p>
                      <Icon className="section-icon" aria-hidden="true" size={28} />
                      <h3>{title}</h3>
                      <p>{body}</p>
                      <div className="step-result">
                        <span>{artifact}</span>
                        <strong>{result}</strong>
                      </div>
                    </div>
                    <div className="step-card-mock" aria-hidden="true">
                      {visual}
                    </div>
                  </article>
                );
              },
            )}
          </div>
        </section>
      </article>
    </Shell>
  );
}

function HeroVisual() {
  return (
    <div className="cinematic-frame hero-frame">
      <div className="frame-chrome">
        <span>Proofplane workspace</span>
        <span>Workspace setup</span>
      </div>
      <div className="stage-grid">
        <div className="stage-panel stage-panel-primary">
          <span>Next action</span>
          <strong>Create a workspace</strong>
          <small>Owner: workspace admin</small>
        </div>
        <div className="stage-panel">
          <span>Token</span>
          <strong>Scoped API access</strong>
          <small>Explicit permission strings</small>
        </div>
        <div className="stage-panel">
          <span>Placeholder</span>
          <strong>MCP and packet views</strong>
          <small>Not implemented yet</small>
        </div>
      </div>
      <div className="stage-command">
        <span>API surface</span>
        <code>Create token for Acme Security</code>
      </div>
      <div className="stage-footer">
        <span>Workspace</span>
        <span>Token</span>
        <span>Evidence</span>
        <span>Controls</span>
      </div>
    </div>
  );
}

function WorkspaceStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-document-stack">
        <div className="mock-upload-card">
          <span>Workspace</span>
          <strong>Acme Security</strong>
          <small>Workspace created</small>
        </div>
        <PlaceholderRow />
        <PlaceholderRow />
      </div>
    </div>
  );
}

function PermissionStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-strategy-panel">
        <h4>Token permissions</h4>
        {["read_controls", "write_evidence_submissions", "read_evidence_requests"].map((label) => (
          <div className="mock-strategy-row" key={label}>
            <span>{label}</span>
            <strong>Scoped</strong>
          </div>
        ))}
      </div>
    </div>
  );
}

function DataApiStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-terminal-panel">
        <span>Backend routes</span>
        <code>{`List Acme Security controls
List access review evidence requests
Submit evidence for quarterly access review`}</code>
      </div>
      <div className="mock-prompt-card">Detailed UI views are not built yet.</div>
    </div>
  );
}

function PlaceholderStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-packet-panel">
        <h4>Later screen</h4>
        {[
          ["MCP tools", "Later"],
          ["Auditor packet preview", "Later"],
          ["Gap summary", "Later"],
        ].map(([label, status]) => (
          <div className="mock-packet-row" key={label}>
            <span>{label}</span>
            <strong>{status}</strong>
          </div>
        ))}
      </div>
    </div>
  );
}

function PlaceholderRow() {
  return (
    <div className="mock-upload-row">
      <span>Placeholder</span>
      <strong>Not loaded</strong>
    </div>
  );
}
