import { Bot, FileCheck2, KeyRound, Layers3 } from "lucide-react";
import { Link } from "react-router-dom";
import { StartSandboxButton } from "../auth/StartSandboxButton";
import { Shell } from "../components/Shell";

const sections = [
  {
    eyebrow: "Step 01",
    title: "Create the workspace.",
    body: "Start with a tenant boundary, starter SOC 2 controls, and a place where compliance work has an owner.",
    icon: Layers3,
    artifact: "Workspace boundary",
    result: "Starter controls loaded",
    summary: "Tenant and controls",
    visual: <WorkspaceStepVisual />,
  },
  {
    eyebrow: "Step 02",
    title: "Pick the job the token should do.",
    body: "Use permission presets for real work: read compliance data, submit evidence, manage mappings, or run the sandbox demo.",
    icon: KeyRound,
    artifact: "Scoped token",
    result: "5 permissions selected",
    summary: "Permission preset",
    visual: <PermissionStepVisual />,
  },
  {
    eyebrow: "Step 03",
    title: "Let agents inspect the right records.",
    body: "MCP setup stays explicit: the token belongs to the session, not the prompt, and preview labels stay honest.",
    icon: Bot,
    artifact: "MCP setup preview",
    result: "Agent prompt ready",
    summary: "Agent context",
    visual: <McpStepVisual />,
  },
  {
    eyebrow: "Step 04",
    title: "See what an auditor still needs.",
    body: "Packet readiness connects controls, evidence requests, latest submissions, provenance, and gaps in one readable artifact.",
    icon: FileCheck2,
    artifact: "Auditor packet",
    result: "Evidence gaps ranked",
    summary: "Gap analysis",
    visual: <PacketStepVisual />,
  },
];

export function HomeRoute() {
  return (
    <Shell>
      <StartSandboxButton className="scroll-cta" />

      <article className="landing-scroll">
        <section className="landing-hero scroll-section" aria-labelledby="home-title">
          <div className="section-copy">
            <p className="eyebrow">SOC 2 compliance infrastructure</p>
            <h1 id="home-title">Compliance tasks, reduced to the next action.</h1>
            <p className="lede">
              Proofplane turns workspace setup, scoped credentials, agent access,
              and auditor packet progress into a guided SOC 2 flow.
            </p>
            <div className="actions">
              <StartSandboxButton />
              <Link className="button button-secondary" to="/pricing">
                Pricing philosophy
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
              <h2 id="step-card-flow-title">Four steps from setup to packet clarity.</h2>
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
        <span>SOC 2 sandbox</span>
      </div>
      <div className="stage-grid">
        <div className="stage-panel stage-panel-primary">
          <span>Next action</span>
          <strong>Create access review evidence</strong>
          <small>Owner: workspace admin</small>
        </div>
        <div className="stage-panel">
          <span>Token</span>
          <strong>Sandbox demo access</strong>
          <small>5 scoped grants</small>
        </div>
        <div className="stage-panel">
          <span>Packet gap</span>
          <strong>Quarterly review missing</strong>
          <small>Ready for agent prompt</small>
        </div>
      </div>
      <div className="stage-command">
        <span>Ask Proofplane</span>
        <code>What evidence is missing for SOC 2?</code>
      </div>
      <div className="stage-footer">
        <span>Workspace</span>
        <span>Token</span>
        <span>MCP</span>
        <span>Packet</span>
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
          <small>SOC 2 starter controls loaded</small>
        </div>
        <div className="mock-upload-row">
          <span>MFA enforced</span>
          <strong>Control ready</strong>
        </div>
        <div className="mock-upload-row">
          <span>Access review</span>
          <strong>Evidence due</strong>
        </div>
      </div>
    </div>
  );
}

function PermissionStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-strategy-panel">
        <h4>Token preset</h4>
        {["Read compliance data", "Submit evidence", "Sandbox access"].map((label) => (
          <div className="mock-strategy-row" key={label}>
            <span>{label}</span>
            <strong>Scoped</strong>
          </div>
        ))}
      </div>
    </div>
  );
}

function McpStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-terminal-panel">
        <span>MCP setup preview</span>
        <code>{`request: quarterly_access_review
scope: read_controls + submit_evidence
status: packet_gap_visible`}</code>
      </div>
      <div className="mock-prompt-card">What evidence is missing for SOC 2?</div>
    </div>
  );
}

function PacketStepVisual() {
  return (
    <div className="step-mock-shell">
      <div className="mock-window-bar" />
      <div className="mock-packet-panel">
        <h4>Auditor packet</h4>
        {[
          ["MFA enforced", "Current"],
          ["Access review", "Missing latest evidence"],
          ["Vendor review", "Mapped"],
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
