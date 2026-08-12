# Auth0 management identity claims

Management-plane access tokens may carry a mailbox identity for operations that
need to bind the authenticated person to an email address, such as accepting a
workspace invitation. This identity is optional: tokens without these claims
continue to authenticate and provision users normally.

An Auth0 Action that runs during the access-token flow must set both of these
namespaced custom claims from the Auth0 user profile:

- `https://proofplane.com/email`: the user's email string.
- `https://proofplane.com/email_verified`: the boolean value of the user's
  verified-email status.

For example, the Action logic can assign the profile values directly:

```js
api.accessToken.setCustomClaim("https://proofplane.com/email", event.user.email);
api.accessToken.setCustomClaim(
  "https://proofplane.com/email_verified",
  event.user.email_verified,
);
```

Proofplane treats the email as authority only when the email claim is a
non-blank, single-`@` string and the verification claim is the boolean `true`.
It trims and lowercases the accepted value. Missing, malformed, or unverified
claims produce no verified management identity; an operation that requires one
must reject the request at that boundary. The ordinary `email` profile claim and
`login_hint` are not authority for this purpose.

Do not place tenant secrets, Action secrets, client secrets, or access tokens in
this document or in the Action source.
