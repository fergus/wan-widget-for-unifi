# Releasing

How a release goes from the main branch to an installed copy updating itself. Most of it
is a checklist; the script and the Release workflow do the parts that are easy to get
wrong.

## What is automated

| Step | By | What it does |
|---|---|---|
| `scripts/prepare-release.ps1` | You, locally | Bumps the version in `src-tauri/Cargo.toml` and `Cargo.lock`, dates the changelog heading, runs fmt, clippy and every test |
| CI (`ci.yml`) | Pull requests and pushes to `main` | The same checks on Linux (widget-core, UI) and Windows (the app, with `--locked`) |
| Release (`release.yml`) | Pushing a `v*` tag | Refuses a tag that differs from the app version, a missing changelog entry, or a missing signing key. Then builds the installer and the signed update, and opens a **draft** release with the changelog entry as its notes |

Installed copies find updates through `latest.json` on the latest published release
(the endpoint in `src-tauri/tauri.conf.json`), so nothing reaches users until you
publish the draft.

## One-time setup

- The `TAURI_SIGNING_PRIVATE_KEY` repository secret holds the updater's private key. Its
  public half is the `pubkey` in `src-tauri/tauri.conf.json`. Losing the private key
  means installed copies can never verify another update, so keep a backup outside
  GitHub.

## Choosing the version

Versions follow [semantic versioning](https://semver.org/): a new feature is a minor
release (0.2.0), a fix only is a patch (0.2.1). The app version lives only in
`src-tauri/Cargo.toml`; `widget-core` has its own version and does not change with it.

## Steps

### 1. Keep the changelog current

Describe each change under `## Unreleased` at the top of `CHANGELOG.md` as it lands, in
plain language for someone who uses the widget. If the heading is missing, add it above
the latest release.

### 2. Prepare on a branch

```powershell
git switch main; git pull
git switch -c release-0.3.0
.\scripts\prepare-release.ps1 0.3.0
```

The script stops if you are on `main`, the tree is dirty, there is no `Unreleased`
section or it is empty, or the version is not newer. It leaves the edits uncommitted so
you can read the diff.

### 3. Docs

If the UI changed, update `README.md` and the screenshots in `docs/images/`:

- Open the preview (the `ui-preview` launch config in `.claude/launch.json`, or
  `python -m http.server 5317 --directory ui`). The page uses synthetic data and accepts `?theme=dark`,
  `?theme=light`, `?away` and `?nowan`.
- Capture at 2x so the images stay sharp. With the preview running, headless Edge can
  render a transparent PNG. The graph places samples by time across its 5-minute
  window, so give it 5 minutes of virtual time to fill (`--virtual-time-budget=300000`):

  ```powershell
  & "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --headless=new --disable-gpu --hide-scrollbars --default-background-color=00000000 --force-device-scale-factor=2 --window-size=360,206 --virtual-time-budget=300000 --user-data-dir=$env:TEMP\edge-shots --screenshot=docs\images\widget-dark.png "http://localhost:5317/?theme=dark"
  ```

  Use a new `--user-data-dir` each time, or Edge serves a cached stylesheet. The
  window height must match the card: 206 for the normal card, 227 for Away.
- Re-read the README against what changed. Check the troubleshooting table still
  matches the messages in `src-tauri/src/errors.rs`.

### 4. Pull request

```powershell
git add -A; git commit -m "Prepare 0.3.0"
git push -u origin release-0.3.0
gh pr create --base main
```

Wait for all CI jobs to pass.

### 5. Check it for real

CI cannot cover these, so do them on a built copy (`cargo tauri dev` from `src-tauri`,
or the installer from a previous draft release) against your own gateway:

- Numbers match the UniFi app: latency, uptime, and the month figure.
- Hover every field and read the tooltip.
- Join another network: the widget goes Away, then resumes when you return.
- Reboot the gateway with the PC still on the home network: the widget backs off and
  recovers, and does not go Away.
- Wrong API key and wrong site give the plain-English messages in the README.

Close an installed copy before running a dev build; the single-instance check makes the
second one exit.

### 6. Merge and tag

Merge the pull request, then tag the merge commit on `main`:

```powershell
git switch main; git pull
git tag v0.3.0
git push origin v0.3.0
```

The tag must be `v` plus the version in `src-tauri/Cargo.toml`. Never reuse or move a
tag that has been pushed.

### 7. Publish the draft

The Release workflow opens a draft release. Before publishing, check it has:

- the notes from `CHANGELOG.md`,
- the `-setup.exe` installer and its `.sig`,
- `latest.json`.

Publish it. That is the moment installed copies can see the update.

### 8. Check the update

On a machine running the previous version, choose **Check for updates** in the tray
menu. It should offer the new version, install it and restart. The log (Settings >
Open log folder) records the result of each check. If nothing is offered, open
`https://github.com/fergus/wan-widget-for-unifi/releases/latest/download/latest.json`
and check the version and that the signature is present.

## If a release is bad

- **Before publishing:** delete the draft and the tag (`git push origin :refs/tags/v0.3.0`
  and `git tag -d v0.3.0`), fix on a branch and start again from step 2 with the same
  version.
- **After publishing:** unpublish or delete the release so `latest` points back at the
  previous one, then ship a patch version (0.3.1) with the fix. Copies that already
  updated stay on the bad version until the patch lands, so do not reuse the tag.
