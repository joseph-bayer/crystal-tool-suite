// A stand-in tool for testing the launcher. It serves one page on PORT, starts a child process
// (so stopping the tool has a process tree to end), and logs what its page could call through
// Tauri's IPC, which should be nothing.
import { spawn } from "node:child_process";
import { createServer } from "node:http";

const port = Number(process.env.PORT);
if (!port) {
	console.error("PORT isn't set");
	process.exit(1);
}

// Lingers until killed; marked so a test can find it.
spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)", "fake-tool-child"], { stdio: "ignore" });

const page = `<!doctype html>
<meta charset="utf-8">
<title>Fake Tool</title>
<h1>Fake Tool</h1>
<p id="ipc">Checking IPC...</p>
<script>
(async () => {
	const tauri = window.__TAURI_INTERNALS__;
	const results = {};
	if (!tauri) {
		results.internals = "absent";
	} else {
		for (const cmd of ["list_tools", "start_tool", "open_logs", "plugin:opener|open_url", "plugin:event|emit"]) {
			try {
				await tauri.invoke(cmd, { id: "fake-tool", url: "https://example.com", event: "x" });
				results[cmd] = "ALLOWED";
			} catch (e) {
				results[cmd] = "denied: " + String(e);
			}
		}
	}
	document.getElementById("ipc").textContent = JSON.stringify(results);
	await fetch("/report", { method: "POST", body: JSON.stringify(results) });
})();
</script>`;

createServer((req, res) => {
	if (req.method === "POST" && req.url === "/report") {
		let body = "";
		req.on("data", (chunk) => (body += chunk));
		req.on("end", () => {
			console.log("ipc check: " + body);
			res.end();
		});
		return;
	}
	res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
	res.end(page);
}).listen(port, "127.0.0.1", () => console.log(`fake tool on http://127.0.0.1:${port}/`));
