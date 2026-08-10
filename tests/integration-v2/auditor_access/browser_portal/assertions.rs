use super::*;

pub(super) fn body_read_model(html: &str) -> String {
    let start = html.find("<body>\n").expect("page body opens") + "<body>\n".len();
    let end = html[start..]
        .find("\n</body>")
        .map(|offset| start + offset)
        .expect("page body closes");
    html[start..end].to_owned()
}

pub(super) fn invite_body(workspace_id: Uuid, token: &str, auditor_email: &str) -> String {
    format!(
        r#"<main class="narrow">
<p class="eyebrow">Auditor verification</p>
<h1>Verify access for {auditor_email}</h1>
<p class="lede">Proofplane will verify this email before opening the read-only evidence portal.</p>

<form class="panel form-panel" method="post" action="/auditor-access/{workspace_id}/login">
<input type="hidden" name="token" value="{token}">
<button type="submit">Continue</button>
</form>
</main>"#
    )
}

pub(super) fn portal_bar(
    workspace_name: &str,
    auditor_email: &str,
    policies_current: bool,
) -> String {
    let (framework_current, policy_current) = if policies_current {
        ("", r#" class="current" aria-current="page""#)
    } else {
        (r#" class="current" aria-current="page""#, "")
    };
    format!(
        r##"<a class="skip-link" href="#main-content">Skip to main content</a><header class="portal-bar"><a class="portal-brand" href="/auditor-access/portal" aria-label="Proofplane auditor portal"><span aria-hidden="true">◇</span>Proofplane</a><div class="portal-identity"><span>{workspace_name}</span><span class="readonly">Read-only</span><span class="auditor-email">{auditor_email}</span><form method="post" action="/auditor-access/logout"><button class="sign-out" type="submit">Sign out</button></form></div></header><nav class="portal-nav" aria-label="Auditor portal"><div><a href="/auditor-access/portal"{framework_current}>Framework requirements</a><a href="/auditor-access/portal/policies"{policy_current}>Policies</a></div></nav>"##
    )
}

pub(super) fn portal_body(
    workspace_name: &str,
    auditor_email: &str,
    requirement_count: usize,
    control_count: usize,
    rows: &str,
) -> String {
    format!(
        r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><span aria-current="page">Framework requirements</span></nav>
<header class="page-header">
<div><p class="eyebrow">Auditor portal</p><h1>Framework requirements</h1><p class="lede">Trace each requirement to its controls and submitted evidence.</p></div>
<dl class="page-stats"><div><dt>Requirements</dt><dd>{requirement_count}</dd></div><div><dt>Controls</dt><dd>{control_count}</dd></div></dl>
</header>

<section class="framework-section" id="framework-soc2" aria-labelledby="framework-title-soc2"><div class="section-heading"><p class="eyebrow">Framework</p><h2 id="framework-title-soc2">SOC 2</h2><p>soc2</p></div><table class="ledger"><thead><tr><th>Requirement</th><th class="numeric">Mapped controls</th><th class="numeric">Evidence</th><th class="numeric">Evidence submissions</th><th>Coverage</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{rows}</tbody></table></section>
</main>"#,
        portal_bar(workspace_name, auditor_email, false),
    )
}

pub(super) fn requirement_row(
    requirement: &TestFrameworkRequirement,
    controls: usize,
    evidence: usize,
    submissions: usize,
    tone: &str,
    coverage: &str,
) -> String {
    format!(
        r#"<tr class="linked-row"><td data-label="Requirement"><a class="row-link" href="/auditor-access/portal/framework-requirements/{}"><strong>{}</strong><span>{}</span></a></td><td class="numeric" data-label="Mapped controls">{controls}</td><td class="numeric" data-label="Evidence">{evidence}</td><td class="numeric" data-label="Evidence submissions">{submissions}</td><td data-label="Coverage"><span class="coverage {tone}">{coverage}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#,
        requirement.id, requirement.code, requirement.title,
    )
}

pub(super) fn requirement_body(
    workspace_name: &str,
    auditor_email: &str,
    requirement: &TestFrameworkRequirement,
    control_count: usize,
    rows: &str,
) -> String {
    format!(
        r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal">Framework requirements</a><span aria-hidden="true">›</span><span>SOC 2</span><span aria-hidden="true">›</span><span aria-current="page">{}</span></nav>
<header class="detail-header"><p class="eyebrow">Framework requirement</p><h1><span class="detail-code">{}</span>{}</h1><p class="lede">{}</p></header>
<div class="requirement-layout">
<aside class="context-panel" aria-labelledby="context-title"><h2 id="context-title">Requirement context</h2><dl><div><dt>Framework</dt><dd>SOC 2</dd></div><div><dt>Framework code</dt><dd>soc2</dd></div><div><dt>Requirement</dt><dd>{}</dd></div></dl></aside>
<section class="detail-ledger" aria-labelledby="controls-title"><div class="section-heading"><p class="eyebrow">Mapped controls</p><h2 id="controls-title">Controls ({control_count})</h2></div><table class="ledger control-ledger"><thead><tr><th>Control</th><th class="numeric">Evidence</th><th class="numeric">Submissions</th><th>Coverage</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{rows}</tbody></table></section>
</div></main>"#,
        portal_bar(workspace_name, auditor_email, false),
        requirement.code,
        requirement.code,
        requirement.title,
        requirement.description,
        requirement.code,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn control_row(
    requirement_id: Uuid,
    control_id: Uuid,
    code: &str,
    title: &str,
    evidence: usize,
    submissions: usize,
    tone: &str,
    coverage: &str,
) -> String {
    format!(
        r#"<tr class="linked-row"><td data-label="Control"><a class="row-link" href="/auditor-access/portal/framework-requirements/{requirement_id}/controls/{control_id}"><strong>{code}</strong><span>{title}</span></a></td><td class="numeric" data-label="Evidence">{evidence}</td><td class="numeric" data-label="Evidence submissions">{submissions}</td><td data-label="Coverage"><span class="coverage {tone}">{coverage}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn control_body(
    workspace_name: &str,
    auditor_email: &str,
    requirement: Option<(Uuid, &str)>,
    _control_id: Uuid,
    code: &str,
    title: &str,
    description: &str,
    evidence_count: usize,
    submission_count: usize,
    policy_count: usize,
    policies: &str,
    evidence: &str,
) -> String {
    let breadcrumbs = match requirement {
        Some((requirement_id, requirement_code)) => format!(
            r#"<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal">Framework requirements</a><span aria-hidden="true">›</span><a href="/auditor-access/portal/framework-requirements/{requirement_id}">{requirement_code}</a><span aria-hidden="true">›</span><span aria-current="page">{code}</span></nav>"#
        ),
        None => format!(
            r#"<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal">Framework requirements</a><span aria-hidden="true">›</span><span aria-current="page">{code}</span></nav>"#
        ),
    };
    format!(
        r#"{}<main class="portal" id="main-content">
{breadcrumbs}
<header class="control-detail-header"><div><p class="eyebrow">Control</p><h1><span class="detail-code">{code}</span>{title}</h1><p class="lede">{description}</p></div><dl class="page-stats"><div><dt>Evidence</dt><dd>{evidence_count}</dd></div><div><dt>Evidence submissions</dt><dd>{submission_count}</dd></div></dl></header>
<section class="attached-policies" aria-labelledby="attached-policies-title"><div class="section-heading"><p class="eyebrow">Policy mappings</p><h2 id="attached-policies-title">Attached policies ({policy_count})</h2></div>{policies}</section>
<section class="evidence-dossier" aria-labelledby="evidence-title"><div class="section-heading"><p class="eyebrow">Evidence</p><h2 id="evidence-title">Submission history</h2></div>{evidence}</section>
</main>"#,
        portal_bar(workspace_name, auditor_email, false),
    )
}

pub(super) fn no_policies() -> String {
    r#"<div class="empty-state"><h3>No policies attached</h3><p>No active policies are attached to this control.</p></div>"#.to_owned()
}

pub(super) fn no_evidence() -> String {
    r#"<div class="empty-state"><h2>No evidence mapped</h2><p>No evidence is mapped to this control.</p></div>"#.to_owned()
}

pub(super) fn policies_body(
    workspace_name: &str,
    auditor_email: &str,
    policy_count: usize,
    mapping_count: usize,
    rows: &str,
) -> String {
    format!(
        r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><span aria-current="page">Policies</span></nav>
<header class="page-header">
<div><p class="eyebrow">Auditor portal</p><h1>Policies</h1><p class="lede">Review policies, their control mappings, and current document availability.</p></div>
<dl class="page-stats"><div><dt>Policies</dt><dd>{policy_count}</dd></div><div><dt>Mapped controls</dt><dd>{mapping_count}</dd></div></dl>
</header>
<table class="ledger policy-ledger"><thead><tr><th>Policy</th><th class="numeric">Mapped controls</th><th>Document</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{rows}</tbody></table>
</main>"#,
        portal_bar(workspace_name, auditor_email, true),
    )
}

pub(super) fn policy_row(
    policy_id: Uuid,
    name: &str,
    controls: usize,
    tone: &str,
    status: &str,
) -> String {
    format!(
        r#"<tr class="linked-row"><td data-label="Policy"><a class="row-link policy-row-link" href="/auditor-access/portal/policies/{policy_id}"><strong>{name}</strong><small class="clamped-description">No description</small></a></td><td class="numeric" data-label="Mapped controls">{controls}</td><td data-label="Document"><span class="coverage {tone}">{status}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn policy_body(
    workspace_name: &str,
    auditor_email: &str,
    policy_id: Uuid,
    name: &str,
    description: &str,
    status: &str,
    document: Option<(Uuid, &str)>,
    control_href: &str,
    control_code: &str,
    control_title: &str,
) -> String {
    let action = document.map_or_else(String::new, |(document_id, filename)| {
        format!(
            r#"<a class="button" href="/auditor-access/portal/policies/{policy_id}/documents/{document_id}/download" aria-label="Download policy document {filename}">Download document</a>"#
        )
    });
    format!(
        r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal/policies">Policies</a><span aria-hidden="true">›</span><span aria-current="page">{name}</span></nav>
<header class="control-detail-header"><div><p class="eyebrow">Policy</p><h1>{name}</h1><p class="lede">{description}</p></div><div class="header-aside"><dl class="page-stats"><div><dt>Mapped controls</dt><dd>1</dd></div><div><dt>Document</dt><dd class="status-stat">{status}</dd></div></dl>{action}</div></header>
<section class="detail-ledger" aria-labelledby="policy-controls-title"><div class="section-heading"><p class="eyebrow">Control mappings</p><h2 id="policy-controls-title">Mapped controls (1)</h2></div><table class="ledger control-ledger"><thead><tr><th>Control</th><th><span class="sr-only">Open</span></th></tr></thead><tbody><tr class="linked-row"><td data-label="Control"><a class="row-link" href="{control_href}"><strong>{control_code}</strong><span>{control_title}</span></a></td><td class="row-arrow" aria-hidden="true">›</td></tr></tbody></table></section>
</main>"#,
        portal_bar(workspace_name, auditor_email, true),
    )
}

pub(super) fn attached_policies(policies: &[(Uuid, &str, &str, &str)]) -> String {
    let rows = policies
        .iter()
        .map(|(policy_id, name, tone, status)| {
            format!(
                r#"<tr class="linked-row"><td data-label="Policy"><a class="row-link policy-row-link" href="/auditor-access/portal/policies/{policy_id}"><strong>{name}</strong><small class="clamped-description">No description</small></a></td><td data-label="Document"><span class="coverage {tone}">{status}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<table class="ledger policy-ledger"><thead><tr><th>Policy</th><th>Document</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{rows}</tbody></table>"#
    )
}

pub(super) fn empty_policies_body(workspace_name: &str, auditor_email: &str) -> String {
    format!(
        r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><span aria-current="page">Policies</span></nav>
<header class="page-header">
<div><p class="eyebrow">Auditor portal</p><h1>Policies</h1><p class="lede">Review policies, their control mappings, and current document availability.</p></div>
<dl class="page-stats"><div><dt>Policies</dt><dd>0</dd></div><div><dt>Mapped controls</dt><dd>0</dd></div></dl>
</header>
<div class="empty-state"><h2>No policies available</h2><p>This workspace does not have any active policies to review.</p></div>
</main>"#,
        portal_bar(workspace_name, auditor_email, true),
    )
}

pub(super) fn unavailable_body() -> String {
    r#"<main class="narrow">
<p class="eyebrow">Access unavailable</p>
<h1>This auditor portal is not available</h1>
<p class="lede">The link or session may be expired or revoked. Ask the Proofplane workspace owner for a new auditor access link.</p>
</main>"#
        .to_owned()
}

pub(super) fn policy_row_with_description(
    policy_id: Uuid,
    name: &str,
    description: &str,
    controls: usize,
    tone: &str,
    status: &str,
) -> String {
    format!(
        r#"<tr class="linked-row"><td data-label="Policy"><a class="row-link policy-row-link" href="/auditor-access/portal/policies/{policy_id}"><strong>{name}</strong><small class="clamped-description">{description}</small></a></td><td class="numeric" data-label="Mapped controls">{controls}</td><td data-label="Document"><span class="coverage {tone}">{status}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#
    )
}

pub(super) fn attached_policy_with_description(
    policy_id: Uuid,
    name: &str,
    description: &str,
    tone: &str,
    status: &str,
) -> String {
    format!(
        r#"<table class="ledger policy-ledger"><thead><tr><th>Policy</th><th>Document</th><th><span class="sr-only">Open</span></th></tr></thead><tbody><tr class="linked-row"><td data-label="Policy"><a class="row-link policy-row-link" href="/auditor-access/portal/policies/{policy_id}"><strong>{name}</strong><small class="clamped-description">{description}</small></a></td><td data-label="Document"><span class="coverage {tone}">{status}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr></tbody></table>"#
    )
}

pub(super) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(super) fn evidence_block(
    title: &str,
    description: &str,
    count: usize,
    submissions: &str,
) -> String {
    format!(
        r#"<details class="request-disclosure" open><summary><span class="summary-title"><strong>{title}</strong><small>{description}</small></span><span class="summary-meta"><span>{count} submissions</span><span class="status-chip">active</span><span class="disclosure-action"><span class="when-closed sr-only">Expand evidence</span><span class="when-open sr-only">Collapse evidence</span><span class="disclosure-chevron" aria-hidden="true"></span></span></span></summary><div class="request-body"><table class="submission-table"><caption class="sr-only">Evidence submissions</caption><thead><tr><th>File</th><th>Received</th><th>Valid from</th><th>Valid until</th><th class="action-column">Actions</th></tr></thead><tbody>{submissions}</tbody></table></div></details>"#
    )
}

pub(super) fn submission_row(submission: &TestEvidenceSubmission, eligible: bool) -> String {
    let action = if eligible {
        format!(
            r#"<a class="button icon-button" href="/auditor-access/portal/evidence-submissions/{}/documents/{}/download" aria-label="Download evidence file"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg><span class="sr-only">Download</span></a>"#,
            submission.id, submission.document_id,
        )
    } else {
        r#"<span class="unavailable">Unavailable</span>"#.to_owned()
    };
    format!(
        r#"<tr><td class="filename" data-label="File">{}</td><td data-label="Received">{}</td><td data-label="Valid from">{}</td><td data-label="Valid until">{}</td><td class="action-column" data-label="Actions">{action}</td></tr>"#,
        submission.filename,
        format_received(&submission.received_at),
        submission.valid_from.format("%Y-%m-%d"),
        submission.valid_until.format("%Y-%m-%d"),
    )
}

pub(super) fn format_received(value: &DateTime<FixedOffset>) -> String {
    value.format("%Y-%m-%d %H:%M UTC").to_string()
}
