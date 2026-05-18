<img src="https://r2cdn.perplexity.ai/pplx-full-logo-primary-dark%402x.png" style="height:64px;margin-right:32px"/>

# Give me a summary of what we've just talked about that I can give to codex to give it context

Here’s a concise context summary you can paste to Codex:

I’m exploring a startup in the compliance tooling space as a competitor to Drata, but I don’t want to build a traditional UI-heavy compliance platform. The idea is an API-first and MCP-first product designed to be consumed by the customer’s own AI agents.

Core concept:

- The product would act as a compliance data/evidence backend.
- Customer-owned AI agents would use the API and MCP to:
    - fetch evidence,
    - query control status,
    - retrieve approved source material,
    - assemble questionnaire answers,
    - and log actions for auditability.
- I would not provide the questionnaire drafting layer myself; the customer’s own agent would do that using the information exposed by my tool.

Positioning:

- “Compliance infrastructure for AI agents.”
- “MCP-native evidence and control backend.”
- “Zero-UI compliance layer for autonomous workflows.”

Competitive framing:

- Drata is perceived as clunky and UI-heavy.
- The goal is to win by being simpler, cheaper, and more agent-friendly.
- The differentiation is not human workflow management, but a trusted backend and tool surface for autonomous agents.

Pricing discussion:

- Pure per-API-call pricing seems risky because not all calls have the same cost or value.
- Better approach is a hybrid usage model:
    - small monthly platform fee,
    - included usage,
    - metered charges for expensive operations,
    - enterprise tier for advanced controls.
- Recommended usage metrics:
    - evidence objects ingested,
    - evidence retrievals,
    - control lookups,
    - agent tool sessions,
    - document parsing,
    - cross-system validations.
- Avoid pricing every raw request equally.

Suggested 3-tier pricing structure:

- Starter: around \$499/month.
- Growth: around \$1,999/month.
- Enterprise: custom pricing.
- Keep MCP access bundled into the base platform so it feels easy for developers and agents.

Important product guidance:

- Don’t try to sell “completed questionnaires.”
- Sell the infrastructure that lets a customer’s AI agent safely assemble them from evidence and control data.
- The business should feel like compliance plumbing for autonomous systems, not a compliance ops dashboard.

