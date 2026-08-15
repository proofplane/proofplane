# Auth0 management profile and identity claims

Management-plane access tokens may carry profile fields for displaying users and
a mailbox identity for operations that need to bind the authenticated person to
an email address, such as accepting a workspace invitation. These claims are
optional: tokens without them continue to authenticate and provision users
normally.

An Auth0 Action that runs during the access-token flow must set these
namespaced custom claims from the Auth0 user profile:

- `https://proofplane.com/email`: the user's email string.
- `https://proofplane.com/name`: the user's display name, when present.
- `https://proofplane.com/email_verified`: the boolean value of the user's
  verified-email status.

For example, the Action logic can assign the profile values directly:

```js
api.accessToken.setCustomClaim("https://proofplane.com/email", event.user.email);
if (event.user.name) {
  api.accessToken.setCustomClaim("https://proofplane.com/name", event.user.name);
}
api.accessToken.setCustomClaim(
  "https://proofplane.com/email_verified",
  event.user.email_verified,
);
```

Proofplane prefers the namespaced email and name for the stored user profile and
falls back to ordinary access-token profile claims for compatibility. A later
token with profile values fills or updates the existing user row; a token that
omits them does not erase stored values. The workspace People API exposes these
values as member `email` and `display_name`.

Proofplane treats the email as authority only when the email claim is a
non-blank, single-`@` string and the verification claim is the boolean `true`.
It trims and lowercases the accepted value. Missing, malformed, or unverified
claims produce no verified management identity; an operation that requires one
must reject the request at that boundary. The ordinary `email` profile claim and
`login_hint` are not authority for this purpose.

Do not place tenant secrets, Action secrets, client secrets, or access tokens in
this document or in the Action source.
