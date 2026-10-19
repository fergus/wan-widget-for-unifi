# WAN Widget for UniFi

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/widget-dark.png">
  <img src="docs/images/widget-light.png" width="360" alt="The widget: download 2.0 Mbit/s and upload 1.8 Mbit/s above a live graph, with 463.0 GB used this month and a public IP with a CGNAT badge">
</picture>

A small Windows 11 desktop widget that shows, from your UniFi gateway's local API:

- a live graph of WAN download and upload throughput
- data used so far in your billing month
- your public IP address, with a CGNAT badge when the gateway's WAN address differs

It sits on top of other windows or pins to the desktop, lives in the tray, and can
start with Windows.

> Not affiliated with or endorsed by Ubiquiti Inc. UniFi is a trademark of Ubiquiti Inc.

## Requirements

- Windows 11 (WebView2 is built in)
- A UniFi gateway running UniFi OS with the Network application. Tested on a UCG Ultra
  with Network 10.6. Reports from other consoles and versions are welcome.
- A UniFi API key

## Setup

1. In the UniFi Network app, open **Settings > Control Plane > Integrations** and
   create an API key.
2. Start the widget. On first run it opens Settings: enter the gateway address
   (default `https://192.168.1.1`), paste the API key, and press **Test connection**.
3. Set your billing day so the monthly figure resets when your ISP's month does. The
   month starts at midnight in the gateway's timezone, which the widget reads from the
   gateway. Set a timezone only to override it.

The gateway uses a self-signed certificate. The widget trusts the certificate it sees
on the first connection and refuses a different one later, until you press
**Forget pinned certificate** in Settings.

### Privacy

The widget talks to your gateway and, for the public IP, to `https://api.ipify.org`
every 5 minutes. You can change the lookup address or turn it off in Settings. Nothing
else leaves your network.

## Configuration files

Settings live in `%APPDATA%\io.github.fergus.unifi-widget\settings.json` (see
`crates/widget-core/src/settings.rs` for fields and defaults). The API key is stored
in Windows Credential Manager (service `unifi-widget`), never in that file.

Other files in the same folder, all written by the app:

- `window.json`: window position and the Pin to desktop choice.
- `gateway-pin.json`: SHA-256 of the gateway's certificate, pinned on first connection.

Errors go to a log file in `%LOCALAPPDATA%\io.github.fergus.unifi-widget\logs`
(Settings > Open log folder). Attach it to bug reports; it never contains the API key.

The installer installs per user to `%LOCALAPPDATA%\WAN Widget for UniFi`. Start with
Windows writes an HKCU Run entry that points at the exe it was enabled from, so turn it
off and on again after moving the app.

## Building from source

| Path | What | Builds on |
|---|---|---|
| `crates/widget-core` | Parsing, billing-cycle maths, CGNAT detection, settings validation (no I/O) | Linux and Windows |
| `ui/` | Static HTML/JS front end; opens in a plain browser with synthetic data | Anywhere (Node 22 for tests) |
| `src-tauri/` | Tauri 2 shell: window, poller, tray, settings file, Credential Manager | Windows |

Tests:

```
cargo test            # widget-core
cd ui && npm test     # graph helpers and formatters
```

The Windows app needs Rust, Node, WebView2 and the Visual Studio C++ build tools:

```
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri dev                        # run from source
cargo tauri build                      # NSIS installer in target\release\bundle\nsis
```

To release, set `version` in `src-tauri/tauri.conf.json`, then push a matching tag
(for example `v0.1.0`). The Release workflow builds the installer and opens a draft
GitHub release with it attached.

Design notes, API findings and open risks are in [docs/DESIGN.md](docs/DESIGN.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
