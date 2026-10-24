// Checks the launcher's version is the same everywhere it's written, and, given --tag, that the
// tag names that version.
//
//   node scripts/check-versions.mjs [--tag v1.2.3]

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(join(root, path), "utf8");
const json = (path) => JSON.parse(read(path));

/** The `version` of the first `[package]` table in a Cargo.toml. */
function cargoTomlVersion(text) {
	const pkg = text.split(/^\[package\]\s*$/m)[1]?.split(/^\[/m)[0] ?? "";
	return pkg.match(/^version\s*=\s*"([^"]*)"/m)?.[1];
}

/** The version of `name` in a Cargo.lock. */
function cargoLockVersion(text, name) {
	const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	return text.match(new RegExp(`^name = "${escaped}"\\r?\\nversion = "([^"]*)"`, "m"))?.[1];
}

const pkg = json("package.json");
const lock = json("package-lock.json");
const found = {
	"package.json": pkg.version,
	"package-lock.json": lock.version,
	'package-lock.json packages[""]': lock.packages?.[""]?.version,
	"src-tauri/Cargo.toml": cargoTomlVersion(read("src-tauri/Cargo.toml")),
	"src-tauri/Cargo.lock": cargoLockVersion(read("src-tauri/Cargo.lock"), "crystal-tool-suite"),
	"src-tauri/tauri.conf.json": json("src-tauri/tauri.conf.json").version,
};

const problems = [];
const version = found["package.json"];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
	problems.push(`package.json's version "${version}" isn't of the form 1.2.3`);
}
for (const [where, v] of Object.entries(found)) {
	if (v !== version) problems.push(`${where} says ${v ?? "nothing"}, package.json says ${version}`);
}

const tagAt = process.argv.indexOf("--tag");
if (tagAt !== -1) {
	const tag = process.argv[tagAt + 1];
	if (tag !== `v${version}`) problems.push(`the tag ${tag ?? "(none given)"} isn't v${version}`);
}

if (problems.length) {
	for (const p of problems) console.error(`version mismatch: ${p}`);
	process.exit(1);
}
console.log(`version ${version} everywhere${tagAt !== -1 ? `, matching the tag` : ""}`);
