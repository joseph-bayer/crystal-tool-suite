// Gathers a launcher release's files from a signed `tauri build` into one folder:
//   crystal-tool-suite_<version>_x64-setup.exe   the installer
//   crystal-tool-suite_<version>_x64-setup.exe.sig  its updater signature
//   latest.json                                   the updater feed, pointing at the installer
//   THIRD_PARTY_NOTICES.txt                       the licenses of what the launcher bundles, as
//                                                 the installer puts beside it
//   SHA256SUMS.txt                                checksums of the four above
//
//   node scripts/release-files.mjs --repo owner/name --tag v1.2.3 --out <folder> [--notes-file <file>]
//
// The installer is renamed because GitHub turns the spaces in "Crystal Tool Suite_..." into dots
// when it's uploaded; the signature covers the file's contents, not its name.

import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function arg(name, required = true) {
	const at = process.argv.indexOf(`--${name}`);
	const value = at === -1 ? undefined : process.argv[at + 1];
	if (required && !value) fail(`--${name} is required`);
	return value;
}

function fail(message) {
	console.error(message);
	process.exit(1);
}

const repo = arg("repo");
const tag = arg("tag");
const out = arg("out");
const notesFile = arg("notes-file", false);

const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
const version = conf.version;
if (tag !== `v${version}`) fail(`the tag ${tag} isn't v${version}`);

const nsis = join(root, "src-tauri/target/release/bundle/nsis");
const builtSetup = join(nsis, `${conf.productName}_${version}_x64-setup.exe`);
const builtSig = `${builtSetup}.sig`;
if (!existsSync(builtSetup)) fail(`no installer at ${builtSetup}`);
if (!existsSync(builtSig)) {
	fail(`no signature at ${builtSig}: build with the release config and the signing key`);
}
// Written by tauri build (its beforeBuildCommand) and bundled into the installer from there.
const builtNotices = join(root, "src-tauri/THIRD_PARTY_NOTICES.txt");
if (!existsSync(builtNotices)) fail(`no third-party notices at ${builtNotices}`);

mkdirSync(out, { recursive: true });
const setupName = `crystal-tool-suite_${version}_x64-setup.exe`;
const setup = join(out, setupName);
const sig = `${setup}.sig`;
copyFileSync(builtSetup, setup);
copyFileSync(builtSig, sig);
const notices = join(out, "THIRD_PARTY_NOTICES.txt");
copyFileSync(builtNotices, notices);

const notes = notesFile ? readFileSync(notesFile, "utf8").trim() : "";
const feed = {
	version,
	...(notes ? { notes } : {}),
	pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
	platforms: {
		"windows-x86_64": {
			signature: readFileSync(sig, "utf8").trim(),
			url: `https://github.com/${repo}/releases/download/${tag}/${setupName}`,
		},
	},
};
const latest = join(out, "latest.json");
writeFileSync(latest, `${JSON.stringify(feed, null, 2)}\n`);

const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const sums = [setup, sig, latest, notices].map((p) => `${sha256(p)}  ${basename(p)}\n`).join("");
writeFileSync(join(out, "SHA256SUMS.txt"), sums);

console.log(`release files for ${tag} in ${out}:`);
process.stdout.write(sums);
