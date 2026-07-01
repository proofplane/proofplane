# Auditor Portal Access UX

## Goal

Give auditors a narrow browser experience for verifying access, reviewing
workspace evidence, and downloading eligible attachments. This is not a full
workflow app in v1.

## Browser Flow

1. Auditor opens the invite link.
2. Proofplane shows the intended auditor email and an action to send an OTP.
3. Auditor enters the OTP.
4. Proofplane creates a seven-day session and opens the portal.
5. Auditor reviews controls, mapped evidence requests, submissions, and
   attachment states.
6. Auditor downloads eligible attachments.

## UI Shape

Use simple server-rendered HTML from the Rust API. Keep the page quiet and
scannable:

- compact header with workspace and auditor email;
- controls grouped with mapped evidence requests;
- submissions ordered by received time within each request;
- attachment rows with status and download action when eligible;
- clear expired, revoked, and invalid-link states.

Do not add a separate frontend app for v1. Revisit a SPA only when auditors
need comments, filtering, review status, saved views, or firm branding.

## Security Copy

The portal should name the intended auditor email before OTP verification.
Never show raw invite tokens, session identifiers, object keys, or storage
locations. Expired, revoked, missing, or invalid links should use generic
recovery copy without revealing workspace data.

## Accessibility

Forms must be keyboard usable, status must not rely on color alone, and
download links must have descriptive names. The read-only table/list layout
should remain usable on narrow screens.
