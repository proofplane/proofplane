# 006 - Discovery Metadata And Funnel

**Status:** Todo · **Depends on:** 004, 005 · **Spec:** [spec.md](../spec.md#funnel-events)

**Summary** - Add crawler-facing product metadata and privacy-safe measurement
of the path from landing page to a successful MCP-backed agent interaction.

**Acceptance criteria**

- [ ] Given public routes, when crawlers request discovery files, then valid
  `robots.txt`, `sitemap.xml`, and `llms.txt` reference canonical URLs.
- [ ] Given public content pages, when inspected, then canonical metadata and
  appropriate structured data are present without contradictory product claims.
- [ ] Given the first-run funnel, when milestones occur, then one coarse event
  per milestone is emitted without compliance content, credentials, or tenant
  identifiers in analytics payloads.
- [ ] Given MCP activity milestones, when measured, then analytics receives only
  coarse events such as first successful tool call, first write, and first
  packet preview, never tool arguments or results.
- [ ] Given repeated page rendering or retries, when measured, then event
  duplication is bounded and documented.

**Tasks**

- [ ] Add discovery files and structured metadata.
- [ ] Add a privacy-reviewed analytics adapter and event schema.
- [ ] Instrument public, MCP setup, and coarse tool-activity milestones.
- [ ] Add contract tests for files, metadata, and prohibited payload fields.
