# Crystal Tool Suite

> **Disclaimer:** Generative AI was used as a code assistant for development of this tool.

One place to get, update and start tools for making Pokémon Crystal hacks.

The launcher lists the suite's tools. Click **Install**, and from then on it starts them, keeps
them up to date and lets you roll back an update that broke something. Each tool opens in its own
window. You can add your own favorite tools too, whether a web page or a program on your computer.

**Tools in the suite:**
- [Gen 1 Map Porter](https://github.com/joseph-bayer/gen1-map-porter): bring Red, Blue and Yellow
  maps into a CrystalShireEngine, pokecrystal or Polished Crystal project.

## Installing

Download `crystal-tool-suite_<version>_x64-setup.exe` from
[Releases](https://github.com/joseph-bayer/crystal-tool-suite/releases) and run it. It installs for
your Windows user only, with no admin prompt. It needs Windows 10 or 11.

> The installer isn't code-signed, so Windows may say "Windows protected your PC" the first time.
> Click **More info → Run anyway**.

To uninstall, use Windows' **Apps & features**. Your installed tools, settings and logs stay unless
you tick **Delete the application data**. Your projects are never touched.

## Using it

- **Installing and updating tools.** Each tool is a card. **Install** downloads it and checks it
  against its published checksum before anything is unpacked.
  - When an update is out, the card says so and shows what's new.
  - An update to a tool that's open waits until you close it.
  - **Details → Roll back** returns to the version you had before.
- **Starting tools.** **Start** opens the tool in its own window. Several tools can run at once.
  - Closing a tool's window stops it.
  - Closing the launcher's window keeps it in the system tray. **Quit** from the tray stops
    everything.
- **Recent projects.** The folders your tools have used are listed here, newest first. **Open in
  <tool>** starts that tool straight on that folder.
- **Your own tools.** **Add a custom tool** takes a name, a thumbnail, and either a web address or a
  program on your computer.
  - A web page opens in its own launcher window.
  - A program is started and then left alone. The launcher doesn't stop it when you quit, so an
    editor never loses unsaved work.
  - A few well-known tools are suggested, such as SoupPotato's Gen 2 Attack Animation Editor and
    mauvesea's Siren.
- **When something goes wrong.** **Details → Open logs** shows the tool's output. **Report a bug**
  opens a new GitHub issue on the tool's repo, with its version and yours filled in.
- **Updating the launcher.** On start it checks for a newer launcher. **Update and restart**
  installs it once no tool is running. Launcher updates are signed, and only a correctly signed
  update installs.

## Where things are kept

| What | Where |
|---|---|
| The launcher | `%LOCALAPPDATA%\Crystal Tool Suite\` |
| Installed tools, settings, custom tools | `%APPDATA%\io.github.joseph-bayer.crystal-tool-suite\` |
| Tool logs | `%LOCALAPPDATA%\io.github.joseph-bayer.crystal-tool-suite\logs\` |
| Folders shared by every tool | `%APPDATA%\Crystal Tool Suite\known-paths.json` |

Each tool keeps its own settings, and anything that belongs to a project stays in that project.

## Making a tool for the suite

A suite tool is a program that serves a web page on `http://127.0.0.1:<PORT>/`. The launcher picks
the port and passes it in the `PORT` environment variable. The tool:
- starts its server, and the launcher opens a window on it once the page answers;
- runs in a Windows job object, so closing its window ends it and anything it started;
- gets no access to the launcher from its page.

The tool's folder holds a `manifest.json`:

```json
{
	"id": "my-tool",
	"name": "My Tool",
	"version": "1.0.0",
	"description": "What it does, in one sentence.",
	"icon": "icon.png",
	"entry": { "windows": { "command": "my-tool.exe" } },
	"folders": ["cse-project"],
	"minLauncher": "0.1.0"
}
```

- `id`, `name`, `version` and `entry` are required, and an `id` uses only `a-z`, `0-9` and `-`.
- `entry` can also give `args`, so `{ "command": "node", "args": ["server.js"] }` works while
  developing.
- `folders` lists the kinds of folder the tool works on. For each kind, the launcher offers
  **Open in <tool>** on matching recent projects and opens the page with
  `?project=<folder>&kind=<kind>`.

**Trying a tool while you build it:** list its folder in
`%APPDATA%\io.github.joseph-bayer.crystal-tool-suite\dev-tools.json`, a JSON array of folder
paths. The launcher shows it as a development copy and never installs over it.
`test-tools/fake-tool/` is a minimal example.

**Releasing it:** a release is a zip of the tool's folder, with `manifest.json` at its root, plus a
`SHA256SUMS.txt`. The reusable workflow `.github/workflows/tool-release.yml` builds both from a
version tag. Add the tool to `index.json` in this repo, which every launcher reads, and it appears
for everyone. [`docs/RELEASING.md`](docs/RELEASING.md) has the details and the index format.

## Building from source

Needs Node 22, and Rust (stable, MSVC) with the Visual Studio C++ build tools.

```bash
npm ci
npm run tauri dev     # run it in development
npx tauri build       # the installer, in src-tauri/target/release/bundle/nsis/
```

`cargo` run directly in `src-tauri/` (`cargo test`, `cargo clippy`) needs the page built and the
third-party notices written first: `npm run build && npm run notices`.

Contributors don't need the signing key. Only official releases are signed, by GitHub Actions.
[`docs/RELEASING.md`](docs/RELEASING.md) covers releasing the launcher.

Built with [Tauri](https://tauri.app), React, Tailwind and shadcn/ui.

## License

MIT; see `LICENSE`. Third-party licenses for what the release bundles are in
`THIRD_PARTY_NOTICES.txt`.
