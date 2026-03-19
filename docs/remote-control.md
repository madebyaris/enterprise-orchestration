# Remote Browser Control

The desktop app exposes an embedded control server so the same Mission Control UI can be opened from a phone browser on the local network.

## How pairing works

1. Open the desktop app.
2. Generate a pairing session from the dashboard.
3. The app creates:
   - a short-lived pairing token,
   - a local-network URL,
   - a QR code that encodes the remote URL.
4. Open the QR code from a phone browser.
5. The browser loads the same control UI and authenticates API requests with the pairing token.

## Authorization model

- Requests from `127.0.0.1`, `localhost`, and `::1` are treated as local desktop traffic.
- Non-local API requests must include a valid pairing token.
- Tokens can be passed:
  - in the `x-orch-pairing-token` header, or
  - in the `token` query string parameter used by the remote browser UI.
- Pairing sessions can be revoked, which immediately blocks future remote API calls for that token.

## Current behavior

- Pairings are stored in SQLite.
- Revoked or expired tokens are rejected.
- The control server serves the built control UI and `/assets/*` routes for remote browsers.
- The dashboard can create and revoke pairings directly.

## Security notes

- The current implementation is designed for **local-network remote control**.
- Treat pairing URLs as sensitive bearer tokens.
- Pairings should be short-lived for real-world deployments.
- Future hardening can add:
  - device identity binding,
  - optional PIN confirmation,
  - TLS termination or secure tunneling,
  - stronger operator roles and session auditing.
