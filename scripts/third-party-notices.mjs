// Writes src-tauri/THIRD_PARTY_NOTICES.txt, which the installer puts beside the launcher and the
// release attaches: the license text of every npm package whose files end up in the page, of the
// Rust standard library, and of every Rust crate built into the launcher for Windows.
//
//   npm run notices [-- --if-missing]
//
// tauri build runs it (beforeBuildCommand), and tauri dev with --if-missing: the Tauri build script
// needs the file to exist, as a bundle resource, but a development build doesn't need it current.
// Building the crate with cargo directly needs it too, like the built page (npm run build).
//
// The npm packages are read from Vite's output (JavaScript modules, stylesheets Tailwind pulls in
// and font files), not from package.json, so the many packages only shadcn's CLI uses aren't
// listed. The crates are those `cargo tree` resolves as normal dependencies of the launcher's crate
// for Windows, with the features that build turns on; build-only and dev-only dependencies aren't
// linked in, so aren't listed. `cargo metadata` gives where each one's files are. Crates with the
// same license text share one copy of it.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "vite";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "src-tauri", "THIRD_PARTY_NOTICES.txt");
// The installer is Windows-only, x64.
const TARGET = "x86_64-pc-windows-msvc";
// The names license, copying and notice files go by (LICENSE, LICENSE.md, LICENSE-MIT,
// LICENSE_APACHE-2.0, licence.txt, COPYING, NOTICE, ...).
const LICENSE_FILE = /^(licen[cs]e|copying|notice)([-_.][\w.-]*)?$/i;
const NOT_LICENSE = /\.(c?js|mjs|ts|json|map|rs|toml)$/i;
const RULE = "=".repeat(100);

const mit = (copyright) => `MIT License

Copyright (c) ${copyright}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;
const WEBVIEW2_RS = {
	source: "https://github.com/wravery/webview2-rs/blob/main/LICENSE",
	text: mit("2021 Bill Avery"),
};
// License texts for the packages and crates that ship none, as their repositories give them; used
// only while a package still has no license file of its own. `sameAs` takes the text from another
// crate that's built in too.
const OVERRIDES = {
	"react-remove-scroll-bar": {
		source: "https://github.com/theKashey/react-remove-scroll-bar/blob/master/LICENSE",
		text: mit("2025 Anton Korzunov <thekashey@gmail.com>"),
	},
	"webview2-com": WEBVIEW2_RS,
	"webview2-com-sys": WEBVIEW2_RS,
	"webview2-com-macros": WEBVIEW2_RS,
	// The repository's LICENSE, which alloc-no-stdlib, from the same repository, ships.
	"alloc-stdlib": {
		source: "https://github.com/dropbox/rust-alloc-no-stdlib/blob/master/LICENSE",
		sameAs: "alloc-no-stdlib",
	},
	// Stylo has no license file; its README says "Stylo is licensed under MPL 2.0". The MPL 2.0
	// text is the one servo's cssparser ships.
	selectors: {
		source: "the MPL 2.0 text (https://github.com/servo/stylo: \"Stylo is licensed under MPL 2.0\")",
		sameAs: "cssparser",
	},
};

if (process.argv.includes("--if-missing") && existsSync(out)) process.exit(0);

const npmPackages = await pagePackages();
const crates = rustCrates();
const version = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8")).version;

const header = [
	`Third-party software in Crystal Tool Suite ${version}`,
	"",
	"The launcher is built from the Rust crates below, and its page includes code, styles and fonts",
	"from the npm packages below. Each one's license follows.",
	"",
	`npm packages (${npmPackages.length}):`,
	...npmPackages.map((pkg) => `  ${pkg.name} ${pkg.version} (${pkg.license})`),
	"",
	"The Rust standard library",
	"",
	`Rust crates (${crates.length}):`,
	...crates.map((crate) => `  ${crate.name} ${crate.version} (${crate.license})`),
].join("\n");

const sections = [
	...npmPackages.map((pkg) => section([pkg])),
	rustStdSection(),
	...groupByText(crates).map(section),
];
writeFileSync(out, `${[header, ...sections].join(`\n\n${RULE}\n\n`)}\n`);
console.log(`${out}: ${npmPackages.length} npm packages, ${crates.length} Rust crates`);

/** The npm packages with files in the built page, sorted. */
async function pagePackages() {
	const files = [];
	await build({
		configFile: join(root, "vite.config.ts"),
		root,
		logLevel: "warn",
		build: { write: false },
		plugins: [
			{
				name: "third-party-notices",
				generateBundle(_options, bundle) {
					for (const output of Object.values(bundle)) {
						if (output.type === "chunk") {
							for (const [id, module] of Object.entries(output.modules)) {
								if (module.renderedLength > 0) files.push(id);
							}
						} else {
							// A font or image's source, relative to the page's root.
							for (const name of output.originalFileNames ?? []) files.push(resolve(root, name));
						}
					}
					// The stylesheets Tailwind's plugin inlines into the page's CSS aren't modules of their
					// own; the plugin registers them as files to watch.
					for (const file of this.getWatchFiles()) {
						if (file.endsWith(".css")) files.push(file);
					}
				},
			},
		],
	});

	const packages = new Map();
	for (const file of files) {
		if (!isAbsolute(file) || !resolve(file).split(sep).includes("node_modules")) continue;
		const pkg = packageOf(resolve(file));
		packages.set(pkg.dir, pkg);
	}
	return [...packages.values()].sort(byNameAndVersion);
}

/** The npm package a file under node_modules belongs to. */
function packageOf(file) {
	// The nearest package.json with a name: some packages have bare ones ({"type": "module"}) in
	// their subfolders.
	for (let dir = dirname(file); dir !== dirname(dir); dir = dirname(dir)) {
		const manifest = join(dir, "package.json");
		if (!existsSync(manifest)) continue;
		const pkg = JSON.parse(readFileSync(manifest, "utf8"));
		if (!pkg.name || !pkg.version) continue;
		const repository = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
		const author = typeof pkg.author === "string" ? pkg.author : pkg.author?.name;
		return { dir, name: pkg.name, version: pkg.version, license: npmLicense(pkg), repository, author, files: licenseFiles(dir) };
	}
	throw new Error(`no package.json above ${file}`);
}

function npmLicense(pkg) {
	if (typeof pkg.license === "string") return pkg.license;
	if (pkg.license?.type) return pkg.license.type;
	if (Array.isArray(pkg.licenses)) return pkg.licenses.map((l) => l.type ?? l).join(" OR ");
	return "unknown";
}

/** The crates linked into the launcher on Windows, sorted. */
function rustCrates() {
	const cargo = (args) =>
		execFileSync("cargo", [...args, "--locked"], {
			cwd: join(root, "src-tauri"),
			encoding: "utf8",
			maxBuffer: 256 * 1024 * 1024,
			stdio: ["ignore", "pipe", "inherit"],
		});
	const metadata = JSON.parse(cargo(["metadata", "--format-version", "1", "--filter-platform", TARGET]));
	const packages = new Map(metadata.packages.map((pkg) => [`${pkg.name} ${pkg.version}`, pkg]));
	const launcher = metadata.packages.find((pkg) => pkg.id === metadata.resolve.root);

	// "name vX.Y.Z" per line (a crate seen before is marked (*) and not expanded again).
	const tree = cargo(["tree", "--edges", "normal", "--target", TARGET, "--prefix", "none", "--format", "{p}"]);
	const linked = new Set();
	for (const line of tree.split(/\r?\n/)) {
		const match = line.match(/^(\S+) v(\S+)/);
		if (match && !(match[1] === launcher.name && match[2] === launcher.version)) linked.add(`${match[1]} ${match[2]}`);
	}

	return [...linked]
		.map((key) => {
			const pkg = packages.get(key);
			if (!pkg) throw new Error(`cargo tree lists ${key}, but cargo metadata doesn't`);
			const dir = dirname(pkg.manifest_path);
			const files = licenseFiles(dir);
			if (pkg.license_file) {
				const named = resolve(dir, pkg.license_file);
				if (!files.some((file) => resolve(dir, file) === named)) files.push(pkg.license_file);
			}
			return {
				dir,
				name: pkg.name,
				version: pkg.version,
				license: pkg.license ?? (pkg.license_file ? `see ${pkg.license_file}` : "unknown"),
				repository: pkg.repository ?? undefined,
				author: pkg.authors?.[0],
				files,
			};
		})
		.sort(byNameAndVersion);
}

function byNameAndVersion(a, b) {
	return a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
}

function licenseFiles(dir) {
	return readdirSync(dir)
		.filter((name) => LICENSE_FILE.test(name) && !NOT_LICENSE.test(name) && statSync(join(dir, name)).isFile())
		.sort();
}

function readText(dir, file) {
	return readFileSync(join(dir, file), "utf8").replace(/\r\n?/g, "\n").trimEnd();
}

/**
 * A package or crate's license texts: its own license files, or, for the few that ship none, the
 * text its repository gives (OVERRIDES).
 * @returns {{ title: string, text: string }[]}
 */
function textsOf(item) {
	if (item.files.length) return item.files.map((file) => ({ title: file, text: readText(item.dir, file) }));
	const override = OVERRIDES[item.name];
	if (!override) return [];
	if (override.text) return [{ title: override.source, text: override.text }];
	const donor = crates.find((crate) => crate.name === override.sameAs);
	if (!donor?.files.length) {
		throw new Error(`${item.name}'s license text is taken from ${override.sameAs}, which isn't built in`);
	}
	return donor.files.map((file) => ({ title: override.source, text: readText(donor.dir, file) }));
}

/** Crates sharing the same license text, each group in the order of its first crate. */
function groupByText(list) {
	const groups = new Map();
	for (const item of list) {
		const texts = textsOf(item);
		// Crates with no license text each get their own section, naming their repository.
		const key = texts.length ? texts.map(({ text }) => text).join("\n\0\n") : `\0${item.name} ${item.version}`;
		if (!groups.has(key)) groups.set(key, []);
		groups.get(key).push(item);
	}
	return [...groups.values()];
}

/** One license text, under the packages or crates it covers. */
function section(items) {
	const lines = items.map((item) => `${item.name} ${item.version}`);
	if (items.length === 1) {
		const [item] = items;
		lines.push(`License: ${item.license}`);
		if (item.repository) lines.push(`Repository: ${item.repository}`);
	} else {
		const licenses = [...new Set(items.map((item) => item.license))];
		lines.push(`License: ${licenses.join("; ")}`);
	}
	const texts = textsOf(items[0]);
	if (texts.length === 0) {
		const [item] = items;
		if (item.author) lines.push(`Author: ${item.author}`);
		lines.push("", "This includes no license file; the license above is the one it declares.");
		console.warn(`${item.name} ${item.version} has no license file: only its declared license (${item.license}) is listed`);
	}
	// A group's first item may ship its license file while a later one takes the same text from
	// OVERRIDES; the label only says so when the first doesn't.
	const fromRepository = items[0].files.length === 0;
	for (const { title, text } of texts) {
		const label = fromRepository ? `--- not in the package; from ${title} ---` : `--- ${title} ---`;
		lines.push("", ...(texts.length > 1 || fromRepository ? [label, ""] : []), text);
	}
	return lines.join("\n");
}

/**
 * The Rust standard library, linked into every Rust program: MIT or Apache-2.0, with the license
 * texts the toolchain's rustc component installs (part of every rustup profile, minimal included).
 */
function rustStdSection() {
	const rustc = (args) => execFileSync("rustc", args, { encoding: "utf8" }).trim();
	const version = rustc(["--version"]).split(" ")[1];
	const licenses = join(rustc(["--print", "sysroot"]), "share", "doc", "rust", "licenses");
	for (const file of ["Apache-2.0.txt", "MIT.txt"]) {
		if (!existsSync(join(licenses, file))) throw new Error(`no ${file} in ${licenses}`);
	}
	// The toolchain's MIT.txt is the generic template. The Rust project's own MIT license has no
	// copyright line, so the template's placeholder one is left out.
	const mitText = readText(licenses, "MIT.txt").replace(/^Copyright \(c\) <year> <copyright holders>\n\n/m, "");
	return [
		`Rust standard library ${version}`,
		"License: MIT OR Apache-2.0",
		"Repository: https://github.com/rust-lang/rust",
		"",
		"Copyrights in the Rust Standard Library are retained by their contributors.",
		"",
		"--- Apache-2.0 ---",
		"",
		readText(licenses, "Apache-2.0.txt"),
		"",
		"--- MIT ---",
		"",
		mitText,
	].join("\n");
}
