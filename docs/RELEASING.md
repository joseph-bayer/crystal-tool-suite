# Releasing

How the launcher and the suite's tools are released. The workflows build, sign and attach
everything to a **draft** GitHub Release; publishing it is done by hand.

## The launcher

### One-time setup: the update key

Installed launchers only accept an update signed with the project's private key, and they check it
against the public key built into them. **If the private key or its password is lost, launchers
already installed can never update again**, and a new key would need everyone to reinstall by hand.

1. Generate the key pair on your own computer, choosing a strong password when asked:

   ```bash
   npx tauri signer generate -w "$HOME/.tauri/crystal-tool-suite.key"
   ```

   This writes the private key to `crystal-tool-suite.key` and the public key to
   `crystal-tool-suite.key.pub` beside it.
2. Keep the private key file's contents and its password in your password manager, and an offline
   backup of both (for example on a USB stick kept somewhere safe). Never commit either.
3. In the GitHub repo, **Settings → Secrets and variables → Actions → New repository secret**, add:
   - `TAURI_SIGNING_PRIVATE_KEY`: the whole contents of `crystal-tool-suite.key`;
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password.
4. Paste the whole contents of `crystal-tool-suite.key.pub` (one line of base64) into
   `src-tauri/tauri.conf.json` as `plugins.updater.pubkey`, and commit it. The public key isn't
   secret. Until it's there, the launcher doesn't look for updates of itself, and the release
   workflow stops with an error.

### Releasing a version

1. Bump the version, the same in all three places:
   - `package.json` and `package-lock.json`: `npm version 0.2.0 --no-git-tag-version`;
   - `src-tauri/Cargo.toml`, then `cargo check` in `src-tauri/` so `Cargo.lock` follows;
   - `src-tauri/tauri.conf.json`.
2. Check they match: `node scripts/check-versions.mjs` (CI runs it too).
3. Commit, push, and wait for CI to pass.
4. Tag the commit with the release notes as the tag's message, and push the tag:

   ```bash
   git tag -a v0.2.0 -m "What's new in 0.2.0"
   git push origin v0.2.0
   ```

   The message is shown in the launcher's "Update available" and on the release page. A tag
   without a message (`git tag v0.2.0`) gives a release with no notes.
5. The **Release** workflow builds the installer and makes a draft release `v0.2.0` with:
   - `crystal-tool-suite_0.2.0_x64-setup.exe`, the installer;
   - `crystal-tool-suite_0.2.0_x64-setup.exe.sig`, its update signature;
   - `latest.json`, the update feed;
   - `THIRD_PARTY_NOTICES.txt`, the licenses of the npm packages and Rust crates the launcher
     bundles (the installer also puts it beside the launcher);
   - `SHA256SUMS.txt`.
6. Check the draft (download the installer and try it if you like), then **Publish release**.
   Don't tick "Set as a pre-release": installed launchers read
   `…/releases/latest/download/latest.json`, which follows the latest full release only.

If the workflow fails, or the draft is wrong: delete the draft and the tag
(`git push --delete origin v0.2.0`, `git tag -d v0.2.0`), fix, and tag again.

### Building on your own computer

`npm run tauri build` builds the installer, `src-tauri/target/release/bundle/nsis/Crystal Tool
Suite_<version>_x64-setup.exe`, without the signing key. To build exactly as the release workflow
does, with the update signature, set the key in the environment and add the release config:

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/crystal-tool-suite.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="…"
npx tauri build --bundles nsis --config src-tauri/tauri.release.conf.json
node scripts/release-files.mjs --repo joseph-bayer/crystal-tool-suite --tag v0.2.0 --out "$TEMP/crystal-tool-suite-release"
```

### What the installer does

- Installs for the current user only, into `%LOCALAPPDATA%\Crystal Tool Suite\`, with no
  administrator prompt; adds a Start menu shortcut, and a desktop shortcut if its box on the last
  page is left ticked (a silent install, `/S`, adds both unless given `/NS`).
- Installs Microsoft's WebView2 runtime first if Windows doesn't have it (Windows 11 always does).
- The installer isn't code-signed, so Windows SmartScreen may warn "Windows protected your PC" the
  first time; "More info → Run anyway" goes on.
- Uninstalling (Settings → Apps) removes the launcher and its shortcuts and keeps
  everything else: installed tools, settings and logs. It offers a "Delete the application data"
  box, unticked; ticked, it also deletes the launcher's data folders,
  `%APPDATA%\io.github.joseph-bayer.crystal-tool-suite\` and
  `%LOCALAPPDATA%\io.github.joseph-bayer.crystal-tool-suite\` (installed tools, settings, logs).
  Neither way touches `%APPDATA%\Crystal Tool Suite\` (files the tools share) or your projects.

## The suite index

`index.json` at the root of this repo, on `main`, is the list of tools the launcher offers. The
launcher reads it from
`https://raw.githubusercontent.com/joseph-bayer/crystal-tool-suite/main/index.json` on every start;
a change is live as soon as it's pushed to `main`. Its format is described at the top of
`src-tauri/src/catalog.rs`:

- `tools`: the suite's own tools, each with its `latest` release and optionally its `previous` one
  (offered by "Roll back" when the launcher kept no copy);
- `customTools`: well-known tools the suite doesn't ship, offered as custom tools.

The launcher itself isn't listed: it finds its own updates through `latest.json` on its releases.

## Releasing a suite tool

A tool's release is one zip per platform, `<id>-<version>-<platform>.zip`, holding the tool's
folder with `manifest.json` at its root, and a `SHA256SUMS.txt` beside it. The launcher only
installs it if the zip's checksum matches and the manifest's `id` and `version` match the index.

### With the reusable workflow

This repo's `.github/workflows/tool-release.yml` does the zipping and the draft release for a tool
repo. In the tool's repo, a workflow run on a version tag builds the tool's folder and hands it
over as an artifact:

```yaml
name: Release
on:
  push:
    tags: ["v*"]
jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
      # … build the tool's folder (its .exe, manifest.json and anything else it needs) into dist-tool/
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: tool
          path: dist-tool/
  release:
    needs: build
    permissions:
      contents: write
    uses: joseph-bayer/crystal-tool-suite/.github/workflows/tool-release.yml@<commit sha> # main
    with:
      artifact: tool
```

Pin `<commit sha>` to a commit of this repo. The workflow checks the manifest (`id`, `name`,
`version`, an `entry` for the platform, and that the tag is `v` plus the manifest's version),
writes the zip and `SHA256SUMS.txt`, makes a draft release with the tag's message as its notes,
and prints the release's entry for the index in the run's summary.

### By hand

1. Put the tool's folder together with `manifest.json` at its root, its `version` matching the tag.
2. Zip the folder's *contents* (not the folder itself) as `<id>-<version>-windows.zip`.
3. `sha256sum <id>-<version>-windows.zip > SHA256SUMS.txt` (in PowerShell:
   `Get-FileHash -Algorithm SHA256`, written as `<hash>  <file name>`).
4. Attach both to the tool's GitHub release.

### Listing the release in the index

Once the tool's release is published, edit `index.json` on `main`. For a new tool, add an entry to
`tools`:

```json
{
  "id": "gen1-map-porter",
  "name": "Gen 1 Map Porter",
  "description": "Ports Gen 1 maps into a Crystal project",
  "icon": "<an https address of its icon, or null>",
  "repo": "https://github.com/joseph-bayer/gen1-map-porter",
  "latest": {
    "version": "1.0.0",
    "notesUrl": "https://github.com/joseph-bayer/gen1-map-porter/releases/tag/v1.0.0",
    "downloads": {
      "windows": {
        "url": "https://github.com/joseph-bayer/gen1-map-porter/releases/download/v1.0.0/gen1-map-porter-1.0.0-windows.zip",
        "sha256": "<the zip's SHA-256>"
      }
    }
  }
}
```

For a new version of a tool already listed, move its `latest` to `previous` and put the new release
in `latest` (the reusable workflow's summary prints it). `"notes": "…"` can be added to a release to
show its notes in the launcher; `notesUrl` opens in the browser. Instead of `sha256`, a download can
name its release's checksums file with `"sha256sumsUrl": "…/SHA256SUMS.txt"`. Commit and push to
`main`; launchers pick it up on their next start or "Check for updates".
