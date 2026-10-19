# Security

## Reporting a problem

Please report security problems privately, through
[Report a vulnerability](https://github.com/fergus/wan-widget-for-unifi/security/advisories/new)
on GitHub, not in a public issue. You should get a reply within a week.

Only the latest release is supported. Fixes ship as a new release, which installed
copies pick up through auto-update.

## What the widget does with your secrets

- **API key:** kept in Windows Credential Manager, never in a file. It is sent only to
  the gateway address you configure, as the `X-API-KEY` header. It is never logged.
- **Gateway certificate:** UniFi gateways use self-signed certificates, so the widget
  pins the certificate it sees on first connection (SHA-256 in `gateway-pin.json`)
  and refuses a different one until you choose Forget pinned certificate.
- **Updates:** each release is signed with the project's updater key, and the widget
  checks that signature before installing anything. The installer itself is not yet
  Authenticode-signed.
- **Network:** besides the gateway, the widget contacts only the public IP lookup
  (default `https://api.ipify.org`, can be turned off) and GitHub for updates.

Things worth reporting include a way to get the API key out of the widget or its
files, a way to make it install an unsigned update, or traffic to anywhere not listed
above.
