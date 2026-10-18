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

Not yet built or run. On Windows 11 with Rust, Node and WebView2:

```
cargo install tauri-cli --version "^2"
cd src-tauri
cargo tauri icon path\to\1024px.png    # generates icons/ (required by the bundler)
cargo tauri dev
```

## Configuration

Settings live in `%APPDATA%\local.unifi-widget\settings.json` (see
`crates/widget-core/src/settings.rs` for fields and defaults). The API key is stored
in Windows Credential Manager (service `unifi-widget`), never in that file.
