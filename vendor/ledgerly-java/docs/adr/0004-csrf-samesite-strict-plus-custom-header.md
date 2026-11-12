# CSRF: SameSite=Strict plus a required custom header, no CSRF tokens

The only credential a browser sends on its own is the refresh-token cookie, and it is only accepted by `POST /auth/refresh` and `POST /auth/logout`. Everything else authenticates with an `Authorization: Bearer` header that JavaScript sets explicitly, which a cross-site request cannot do. So Spring Security's CSRF protection is switched off and these two endpoints are protected differently:

- The cookie is `HttpOnly; Secure; SameSite=Strict; Path=/api/v1/auth`. A browser does not attach it to any cross-site request, and never sends it outside the auth path.
- Both endpoints require the header `X-Requested-With: ledgerly`. A plain cross-site HTML form cannot set custom headers, and a cross-origin `fetch` with one triggers a CORS preflight, which the allowlist added in M10 refuses for unknown origins.

Either control alone stops the classic attack; together a mistake in one (an over-permissive CORS rule, an old browser without SameSite) is covered by the other. A synchroniser token would need server-side session state, which this stateless API otherwise avoids.

## Consequences

- Clients must send the header on refresh and logout; a missing header is a 400.
- If an endpoint ever starts to accept the cookie elsewhere, or sets `SameSite=Lax` or `None` (for example for a cross-site frontend), this decision must be revisited and CSRF tokens added.
