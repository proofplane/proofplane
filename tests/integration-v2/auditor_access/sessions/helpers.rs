use axum_test::TestResponse;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::json::{assert_rfc3339, object_keys};

pub(super) enum ResendControl {
    Available,
    Sent,
}

pub(super) fn wrong_otp(code: &str) -> &'static str {
    if code == "000000" {
        "999999"
    } else {
        "000000"
    }
}

#[track_caller]
pub(super) fn assert_verification_page(
    html: &str,
    workspace_id: Uuid,
    invite_token: &str,
    auditor_email: &str,
    notice: Option<&str>,
    resend_control: ResendControl,
) {
    let notice = notice
        .map(|message| {
            format!(
                r#"<section class="notice" role="status"><p>{}</p></section>"#,
                escape_html(message)
            )
        })
        .unwrap_or_default();
    let (resend_class, confirmation, button_label, success_icon) = match resend_control {
        ResendControl::Available => ("resend-action", "", "Resend code", ""),
        ResendControl::Sent => (
            "resend-action sent",
            r#"<span class="resend-confirmation" role="status" aria-live="polite">New code sent.</span>"#,
            "Send again",
            r#"<span class="resend-success-icon" aria-hidden="true">✓</span>"#,
        ),
    };
    let verification_form = format!(
        r#"<form class="panel form-panel" method="post" action="/auditor-access/{workspace_id}/otp/verify/browser">
<input type="hidden" name="token" value="{invite_token}">
<label for="code">Verification code</label>
<input id="code" name="code" inputmode="numeric" autocomplete="one-time-code" pattern="[0-9]{{6}}" maxlength="6" required>
<button type="submit">Open portal</button>
</form>"#
    );
    let resend_form = format!(
        r#"<form method="post" action="/auditor-access/{workspace_id}/otp/request/browser"><input type="hidden" name="token" value="{invite_token}"><input type="hidden" name="resend" value="true"><button class="link-button" type="submit">{button_label}</button></form>"#
    );

    assert_eq!(
        main_content(html),
        format!(
            r#"<main class="narrow">
<p class="eyebrow">Code required</p>
<h1>Enter the code sent to {auditor_email}</h1>
<p class="lede">Codes expire after 10 minutes. A successful check creates a seven-day browser session for this portal only.</p>
{notice}
{verification_form}
<div class="{resend_class}">{confirmation}{resend_form}{success_icon}</div>
</main>"#
        )
    );
    assert_eq!(forms(html), [verification_form, resend_form]);
}

#[track_caller]
pub(super) fn assert_initial_send_failure_page(
    html: &str,
    workspace_id: Uuid,
    invite_token: &str,
    auditor_email: &str,
) {
    let form = format!(
        r#"<form class="panel form-panel" method="post" action="/auditor-access/{workspace_id}/otp/request/browser">
<input type="hidden" name="token" value="{invite_token}">
<button type="submit">Send verification code</button>
</form>"#
    );
    assert_eq!(
        main_content(html),
        format!(
            r#"<main class="narrow">
<p class="eyebrow">Auditor verification</p>
<h1>Verify access for {auditor_email}</h1>
<p class="lede">Proofplane will send a single-use code to this email before opening the read-only evidence portal.</p>
<section class="notice" role="status"><p>We couldn&#39;t send the verification code. Please try again.</p></section>
{form}
</main>"#
        )
    );
    assert_eq!(forms(html), [form]);
}

#[track_caller]
pub(super) fn assert_unavailable_page(html: &str) {
    assert_eq!(
        main_content(html),
        r#"<main class="narrow">
<p class="eyebrow">Access unavailable</p>
<h1>This auditor portal is not available</h1>
<p class="lede">The link or session may be expired or revoked. Ask the Proofplane workspace owner for a new auditor access link.</p>
</main>"#
    );
    assert_eq!(forms(html), Vec::<String>::new());
}

#[track_caller]
pub(super) fn assert_portal_data_not_found(response: TestResponse) {
    response.assert_status_not_found();
    assert_eq!(
        response.json::<Value>(),
        json!({
            "error": {
                "code": "not_found",
                "message": "route not found",
                "details": [],
            }
        })
    );
}

#[track_caller]
pub(super) fn assert_audit_record(
    record: &Value,
    event_name: &str,
    operation: &str,
    workspace_id: Uuid,
    auditor_email: &str,
    expected_object_id: Option<&str>,
) {
    assert_eq!(
        object_keys(record),
        ["fields", "level", "target", "timestamp"]
            .into_iter()
            .collect()
    );
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["target"], "proofplane::audit");
    assert_rfc3339(&record["timestamp"]);

    let fields = &record["fields"];
    assert_eq!(
        object_keys(fields),
        [
            "actor_type",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is text"))
        .expect("event id is a UUID");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "system");
    assert_eq!(fields["system_name"], "auditor_browser");
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    Uuid::parse_str(fields["request_id"].as_str().expect("request id is text"))
        .expect("request id is a UUID");
    assert_eq!(
        fields["metadata"],
        format!(r#"{{"auditor_email":"{auditor_email}"}}"#)
    );
    assert_eq!(fields["object_type"], "auditor_access");
    let object_id = fields["object_id"]
        .as_str()
        .expect("audit object id is text");
    match expected_object_id {
        Some(expected) => assert_eq!(object_id, expected),
        None => {
            Uuid::parse_str(object_id).expect("audit object id is a UUID");
        }
    }
}

fn main_content(html: &str) -> String {
    let start = html.find("<main").expect("page has main content");
    let main = &html[start..];
    let end = main
        .find("</main>")
        .map(|index| index + "</main>".len())
        .expect("main content closes");
    main[..end].to_owned()
}

fn forms(html: &str) -> Vec<String> {
    let mut forms = Vec::new();
    let mut remaining = html;

    while let Some(start) = remaining.find("<form") {
        let form = &remaining[start..];
        let end = form
            .find("</form>")
            .map(|index| index + "</form>".len())
            .expect("form closes");
        forms.push(form[..end].to_owned());
        remaining = &form[end..];
    }

    forms
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
