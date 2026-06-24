import {
  ArrowRight,
  Bot,
  ClipboardCheck,
  FileCheck2,
  KeyRound,
  Layers3,
} from "lucide-react";
import { Link } from "react-router-dom";
import { Shell } from "../components/Shell";

const sections = [
  {
    eyebrow: "Step 01",
    title: "Create the workspace.",
    body: "Start with a tenant boundary, starter SOC 2 controls, and a place where compliance work has an owner.",
    icon: Layers3,
    visual: <WorkspaceVisual />,
  },
  {
    eyebrow: "Step 02",
    title: "Pick the job the token should do.",
    body: "Use permission presets for real work: read compliance data, submit evidence, manage mappings, or run the sandbox demo.",
    icon: KeyRound,
    visual: <PermissionVisual />,
  },
  {
    eyebrow: "Step 03",
    title: "Let agents inspect the right records.",
    body: "MCP setup stays explicit: the token belongs to the session, not the prompt, and preview labels stay honest.",
    icon: Bot,
    visual: <McpVisual />,
  },
  {
    eyebrow: "Step 04",
    title: "See what an auditor still needs.",
    body: "Packet readiness connects controls, evidence requests, latest submissions, provenance, and gaps in one readable artifact.",
    icon: FileCheck2,
    visual: <PacketVisual />,
  },
];

export function HomeRoute() {
  return (
    <Shell>
      <Link className="scroll-cta" to="/app">
        Start SOC 2 Sandbox
        <ArrowRight aria-hidden="true" size={16} />
      </Link>

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
              <Link className="button button-primary" to="/app">
                Start SOC 2 Sandbox
                <ArrowRight aria-hidden="true" size={16} />
              </Link>
            </div>
          </div>
          <div className="sticky-visual" aria-hidden="true">
            <HeroVisual />
          </div>
        </section>

        {sections.map(({ body, eyebrow, icon: Icon, title, visual }) => (
          <section
            className="scroll-section feature-section"
            data-step={eyebrow.replace("Step ", "")}
            key={title}
          >
            <div className="section-copy">
              <p className="eyebrow">{eyebrow}</p>
              <Icon className="section-icon" aria-hidden="true" size={28} />
              <h2>{title}</h2>
              <p>{body}</p>
            </div>
            <div className="sticky-visual" aria-hidden="true">
              {visual}
            </div>
          </section>
        ))}

        <section
          className="final-section scroll-section"
          data-step="05"
          aria-labelledby="final-title"
        >
          <div className="section-copy">
            <p className="eyebrow">Ready path</p>
            <ClipboardCheck className="section-icon" aria-hidden="true" size={28} />
            <h2 id="final-title">Simplify the work, keep the audit trail.</h2>
            <p>
              Every setup step produces something concrete: a workspace, a
              permissioned token, MCP guidance, and a packet view that points to
              the next evidence task.
            </p>
            <div className="actions">
              <Link className="button button-primary" to="/app">
                Start SOC 2 Sandbox
                <ArrowRight aria-hidden="true" size={16} />
              </Link>
            </div>
          </div>
          <div className="sticky-visual" aria-hidden="true">
            <ChecklistVisual />
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

function WorkspaceVisual() {
  return (
    <div className="cinematic-frame workspace-artifact">
      <div className="frame-chrome">
        <span>Workspace</span>
        <span>Owner</span>
      </div>
      <div className="workspace-card stage-card">
        <strong>Acme Security</strong>
        <span>SOC 2 starter controls loaded</span>
      </div>
      <div className="artifact-row">
        <span>MFA</span>
        <strong>Control ready</strong>
      </div>
      <div className="artifact-row">
        <span>Access review</span>
        <strong>Evidence due</strong>
      </div>
    </div>
  );
}

function PermissionVisual() {
  return (
    <div className="cinematic-frame permission-artifact">
      <div className="frame-chrome">
        <span>Token preset</span>
        <span>Scoped</span>
      </div>
      <div className="permission-grid">
        <div>
          <span>Preset</span>
          <strong>Read compliance data</strong>
        </div>
        <div>
          <span>Evidence</span>
          <strong>Submit evidence</strong>
        </div>
        <div>
          <span>Demo</span>
          <strong>Sandbox access</strong>
        </div>
      </div>
      <div className="permission-code">
        <span>Granted permissions</span>
        <code>read_controls, read_evidence_requests</code>
      </div>
    </div>
  );
}

function McpVisual() {
  return (
    <div className="cinematic-frame code-artifact">
      <div className="frame-chrome">
        <span>MCP setup preview</span>
        <span>Preview</span>
      </div>
      <div className="mcp-terminal">
        <span>Install command</span>
        <code>{`PROOFPLANE_TOKEN=ppat_...
proofplane mcp --workspace acme`}</code>
      </div>
      <div className="mcp-flow" aria-hidden="true">
        <div>
          <span>Session token</span>
          <strong>Scoped</strong>
        </div>
        <div>
          <span>Tool access</span>
          <strong>Controls + evidence</strong>
        </div>
        <div>
          <span>Agent prompt</span>
          <strong>SOC 2 gaps</strong>
        </div>
      </div>
      <div className="prompt-card">
        <span>Suggested prompt</span>
        <p>What evidence is missing for SOC 2?</p>
      </div>
    </div>
  );
}

function PacketVisual() {
  return (
    <div className="cinematic-frame packet-artifact">
      <div className="frame-chrome">
        <span>Auditor packet</span>
        <span>Gaps</span>
      </div>
      <div className="artifact-row">
        <span>MFA enforced</span>
        <strong>Current</strong>
      </div>
      <div className="artifact-row signal-row">
        <span>Access review</span>
        <strong>Missing latest evidence</strong>
      </div>
      <div className="artifact-row">
        <span>Vendor review</span>
        <strong>Mapped</strong>
      </div>
    </div>
  );
}

function ChecklistVisual() {
  return (
    <div className="cinematic-frame checklist-artifact">
      <div className="frame-chrome">
        <span>Next actions</span>
        <span>Clear</span>
      </div>
      <ol className="checklist">
        <li>Workspace created</li>
        <li>Token permissioned</li>
        <li>MCP guidance visible</li>
        <li>Evidence gap identified</li>
      </ol>
    </div>
  );
}
