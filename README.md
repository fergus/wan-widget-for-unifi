# UniFi Widget

Windows 11 desktop widget: live WAN upload/download graph, monthly data usage and
external IP, fed by a UniFi gateway's local API. See [docs/PLAN.md](docs/PLAN.md).

## Layout

| Path | What | Builds on |
|---|---|---|
| `crates/widget-core` | Parsing, billing-cycle maths, CGNAT detection, settings validation (no I/O) | Linux + Windows |
| `ui/` | Static HTML/JS front end; opens in a plain browser with synthetic data | Anywhere (Node 22 for tests) |
| `src-tauri/` | Tauri 2 shell: window, poller, settings file, Credential Manager | Windows only (unverified) |

## Tests

```
cargo test            # widget-core
cd ui && npm test     # graph helpers and formatters
```

## Building the Windows app

Built and run on Windows 11 with Rust, Node and WebView2 (Visual Studio C++ build tools
for the MSVC linker):

```
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri icon path\to\1024px.png    # generates icons/ (required by the bundler)
cargo tauri dev                        # run from source
cargo tauri build                      # NSIS installer in target\release\bundle\nsis
```

The tray icon uses the app icon and the light- and dark-taskbar PNGs in `icons/tray/`.
The bundler needs `icons/icon.ico`, so `cargo tauri icon` must have been run.

The installer installs per user to `%LOCALAPPDATA%\UniFi Widget`. Autostart (Start with
Windows) writes an HKCU Run entry that points at the exe it was enabled from, so turn it
off and on again after moving the app.

## Configuration

Settings live in `%APPDATA%\local.unifi-widget\settings.json` (see
`crates/widget-core/src/settings.rs` for fields and defaults). The API key is stored
in Windows Credential Manager (service `unifi-widget`), never in that file.

Other files in the same folder, all written by the app:

- `window.json`: window position and the Pin to desktop choice.
- `gateway-pin.json`: SHA-256 of the gateway's certificate, pinned on first connection.
  If the certificate changes, the widget refuses it until you press Forget pinned
  certificate in Settings.
