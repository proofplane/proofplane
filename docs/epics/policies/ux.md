# Policies UX

## Principles

Policy UI work extends the two existing server-rendered browser surfaces. The
auditor portal should make policies easy to discover and trace to controls; the
delegated attachment page should feel like the existing evidence attachment
manager. Neither surface becomes a general policy administration app.

## Auditor Navigation

Add persistent top-level navigation with two destinations:

- **Framework requirements** — the existing portal landing page and trace from
  requirements to controls and evidence.
- **Policies** — the workspace-wide active policy catalog.

The current destination is visually and programmatically identified. The
navigation remains usable on narrow screens and does not expose workspace IDs,
auditor session identifiers, or invite secrets.

## Policies Catalog

The Policies page lists all active policies in case-insensitive name order.
Each row or card shows:

- policy name;
- an abbreviated description or a clear “No description” state;
- mapped-control count;
- document status, including a clear “No document” state;
- a link to the policy detail page.

There is no search, filter, pagination, inline document preview, or archived
policy view in v1. Empty workspaces show a purposeful read-only empty state
rather than an empty table.

## Policy Detail

The detail page shows the full name and description, an attachment panel, and
the mapped controls. The attachment panel presents filename, size, and coarse
status when an attachment exists. It shows a download action only when the
document is eligible; all other states explain that no download is available
without exposing internal processing details.

Mapped controls show code and title and link back to the existing control
detail route in an appropriate framework context. If a control belongs to
multiple framework requirements, choose a deterministic route; if it belongs
to none, provide a portal control route that does not require inventing a
framework relationship.

## Control Detail

Add an “Attached policies” section before or alongside evidence-request
history. It lists policy names, abbreviated descriptions, and document status,
with each item linking to its policy detail. An empty state explicitly says
that no policies are attached.

## Policy Attachment Management

Mirror the evidence attachment management page's shell, typography, spacing,
status treatment, form behavior, download affordance, error presentation, and
responsive layout. Change only the resource-specific language and rules:

- identify the policy by name;
- allow one file selection and upload only when no active attachment exists;
- show the current attachment and lifecycle status;
- allow archive only for `uploaded`, `contains_virus`, or `failed`;
- allow download only for `uploaded`;
- explain that changing the document requires deleting it and uploading again.

After a successful upload or archive, redirect back to the management page so
refresh does not repeat the mutation. Expired or invalid delegated sessions use
the same generic unavailable treatment as evidence attachment management.

## Accessibility And Safety

Navigation, tables/cards, forms, status text, archive confirmation, and
downloads must be keyboard and screen-reader usable. Status cannot depend on
color alone. Escape policy names, descriptions, control text, and filenames.
Use descriptive download labels and preserve the existing safe response
headers and no-secret rendering rules.
