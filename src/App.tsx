import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Plus, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardFooter } from "@/components/ui/card";
import { CustomToolDialog } from "@/CustomToolDialog";
import { ToolImage } from "@/ToolImage";
import type { CustomTool, Settings, Suggestion, ToolList, ToolRow } from "@/types";

function size(bytes: number) {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
	return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function when(iso: string) {
	const date = new Date(iso);
	return Number.isNaN(date.getTime()) ? iso : date.toLocaleString();
}

type Selected = { type: "tool"; id: string } | { type: "custom"; id: string };
type Call = (cmd: string, args?: Record<string, unknown>) => void;

/** What a tool's card says about it, in a few words. */
function status(tool: ToolRow) {
	if (tool.task) return `${tool.task}...`;
	if (tool.error || tool.installError) return "Something went wrong; see details";
	if (tool.updateWaiting) return "Update waiting for it to close";
	if (tool.state === "running") return "Running";
	if (tool.state === "starting") return "Starting";
	if (tool.version === null) return "Not installed";
	if (tool.updateAvailable) return "Update available";
	return "Ready";
}

/** The one button on a tool's card: what you'd most likely do next. */
function PrimaryButton({ tool, call }: { tool: ToolRow; call: Call }) {
	if (tool.task) return <Button disabled>{tool.task}...</Button>;
	if (tool.version === null)
		return tool.latest ? (
			<Button onClick={() => call("install_tool", { id: tool.id })}>Install {tool.latest.version}</Button>
		) : null;
	if (tool.state === "running") return <Button onClick={() => call("start_tool", { id: tool.id })}>Open</Button>;
	if (tool.state === "starting") return <Button disabled>Starting...</Button>;
	if (tool.updateAvailable && tool.latest && !tool.updateWaiting)
		return (
			<Button onClick={() => call("install_tool", { id: tool.id })}>Update to {tool.latest.version}</Button>
		);
	return <Button onClick={() => call("start_tool", { id: tool.id })}>Start</Button>;
}

function ToolCard({ tool, call, onDetails }: { tool: ToolRow; call: Call; onDetails: () => void }) {
	const failed = tool.error || tool.installError;
	return (
		<Card size="sm">
			<CardContent className="flex gap-3">
				<ToolImage src={tool.icon} />
				<div className="flex min-w-0 flex-col">
					<span className="truncate font-medium">{tool.name}</span>
					<span className="text-xs text-muted-foreground">
						{tool.version !== null ? `Installed ${tool.version}` : "Not installed"}
						{tool.dev && " (development copy)"}
						{tool.latest && ` · Latest ${tool.latest.version}`}
					</span>
					<span className={failed ? "text-xs text-destructive" : "text-xs text-muted-foreground"}>
						{status(tool)}
					</span>
				</div>
			</CardContent>
			<CardFooter className="gap-2">
				<PrimaryButton tool={tool} call={call} />
				<Button variant="ghost" className="ml-auto" onClick={onDetails} aria-label={`Details for ${tool.name}`}>
					Details
				</Button>
			</CardFooter>
		</Card>
	);
}

function ToolDetails({
	tool,
	list,
	call,
	confirmUninstall,
	setConfirmUninstall,
}: {
	tool: ToolRow;
	list: ToolList;
	call: Call;
	confirmUninstall: string | null;
	setConfirmUninstall: (id: string | null) => void;
}) {
	const installed = tool.version !== null;
	const busy = tool.task !== null;
	const stopped = tool.state === "stopped";
	return (
		<>
			{tool.description && <p className="text-muted-foreground">{tool.description}</p>}
			<dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
				<dt className="text-muted-foreground">Installed</dt>
				<dd>
					{installed ? tool.version : "not installed"}
					{tool.dev && " (development copy)"}
				</dd>
				{tool.latest && (
					<>
						<dt className="text-muted-foreground">Latest</dt>
						<dd>{tool.latest.version}</dd>
					</>
				)}
				{tool.keptVersion && (
					<>
						<dt className="text-muted-foreground">Kept</dt>
						<dd>{tool.keptVersion}, for rolling back</dd>
					</>
				)}
				{tool.diskUse !== null && (
					<>
						<dt className="text-muted-foreground">Disk use</dt>
						<dd>{size(tool.diskUse)}</dd>
					</>
				)}
				{installed && (
					<>
						<dt className="text-muted-foreground">Folders</dt>
						<dd>{tool.folders.length > 0 ? tool.folders.join(", ") : "none"}</dd>
					</>
				)}
			</dl>

			{tool.task && <p className="text-muted-foreground">{tool.task}...</p>}
			{tool.updateWaiting && tool.latest && (
				<p className="text-muted-foreground">
					The update to {tool.latest.version} will install when {tool.name} is closed.
				</p>
			)}
			{tool.error && <p className="text-destructive">{tool.error}</p>}
			{tool.installError && <p className="text-destructive">{tool.installError}</p>}
			{tool.dev && tool.latest && (
				<p className="text-muted-foreground">
					The suite index has version {tool.latest.version}. To install it, remove this folder from{" "}
					{list.devToolsFile}.
				</p>
			)}

			<div className="flex flex-wrap gap-2">
				{installed && (
					<Button
						disabled={tool.state === "starting" || busy}
						onClick={() => call("start_tool", { id: tool.id })}
					>
						{tool.state === "running" ? "Open" : tool.state === "starting" ? "Starting..." : "Start"}
					</Button>
				)}
				{!installed && tool.latest && (
					<Button disabled={busy} onClick={() => call("install_tool", { id: tool.id })}>
						Install {tool.latest.version}
					</Button>
				)}
				{tool.updateAvailable && tool.latest && (
					<Button
						variant="outline"
						disabled={busy || tool.updateWaiting}
						onClick={() => call("install_tool", { id: tool.id })}
					>
						Update to {tool.latest.version}
					</Button>
				)}
				{tool.rollBackTo && (
					<Button
						variant="outline"
						disabled={busy || !stopped}
						title={stopped ? undefined : "Close it first"}
						onClick={() => call("roll_back_tool", { id: tool.id })}
					>
						Roll back to {tool.rollBackTo}
					</Button>
				)}
				{installed && (
					<Button
						variant="outline"
						disabled={tool.state !== "running"}
						onClick={() => call("open_in_browser", { id: tool.id })}
					>
						Open in browser
					</Button>
				)}
				{installed && (
					<Button variant="outline" onClick={() => call("open_logs", { id: tool.id })}>
						Open logs
					</Button>
				)}
				{tool.canReport && (
					<Button variant="outline" onClick={() => call("report_bug", { id: tool.id })}>
						Report a bug
					</Button>
				)}
				{installed && !tool.dev && (
					<Button
						variant="destructive"
						disabled={busy || !stopped}
						title={stopped ? undefined : "Close it first"}
						onClick={() =>
							confirmUninstall === tool.id
								? call("uninstall_tool", { id: tool.id })
								: setConfirmUninstall(tool.id)
						}
					>
						{confirmUninstall === tool.id ? "Click again to uninstall" : "Uninstall"}
					</Button>
				)}
			</div>

			{tool.latest && (tool.latest.notes || tool.latest.hasNotesUrl) && (
				<div className="flex flex-col gap-1 rounded-md bg-muted p-2">
					<span className="font-medium">What's new in {tool.latest.version}</span>
					{tool.latest.notes && (
						<pre className="whitespace-pre-wrap font-sans text-muted-foreground">{tool.latest.notes}</pre>
					)}
					{tool.latest.hasNotesUrl && (
						<Button
							variant="link"
							size="sm"
							className="self-start px-0"
							onClick={() => call("open_release_notes", { id: tool.id })}
						>
							Release notes
						</Button>
					)}
				</div>
			)}

			{tool.folder && <p className="break-all text-xs text-muted-foreground">In {tool.folder}</p>}
		</>
	);
}

function CustomCard({ tool, call, onDetails }: { tool: CustomTool; call: Call; onDetails: () => void }) {
	return (
		<Card size="sm">
			<CardContent className="flex gap-3">
				<ToolImage src={tool.image} kind={tool.kind} />
				<div className="flex min-w-0 flex-col">
					<span className="truncate font-medium">{tool.name}</span>
					<span className="text-xs text-muted-foreground">
						{tool.kind === "web" ? "Web page" : "Program"}
					</span>
				</div>
			</CardContent>
			<CardFooter className="gap-2">
				<Button onClick={() => call("open_custom_tool", { id: tool.id })}>Open</Button>
				<Button variant="ghost" className="ml-auto" onClick={onDetails} aria-label={`Details for ${tool.name}`}>
					Details
				</Button>
			</CardFooter>
		</Card>
	);
}

function CustomDetails({
	tool,
	call,
	onEdit,
	confirmRemove,
	setConfirmRemove,
}: {
	tool: CustomTool;
	call: Call;
	onEdit: () => void;
	confirmRemove: string | null;
	setConfirmRemove: (id: string | null) => void;
}) {
	return (
		<>
			<dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
				<dt className="text-muted-foreground">Kind</dt>
				<dd>{tool.kind === "web" ? "Web page, opened in a launcher window" : "Program on this computer"}</dd>
				{tool.url && (
					<>
						<dt className="text-muted-foreground">Address</dt>
						<dd className="break-all">{tool.url}</dd>
					</>
				)}
				{tool.program && (
					<>
						<dt className="text-muted-foreground">Program</dt>
						<dd className="break-all">{tool.program}</dd>
					</>
				)}
				{tool.args && (
					<>
						<dt className="text-muted-foreground">Arguments</dt>
						<dd className="break-all">{tool.args}</dd>
					</>
				)}
			</dl>
			{tool.kind === "program" && (
				<p className="text-muted-foreground">
					The launcher starts it and leaves it running on its own: close it yourself when you're done.
				</p>
			)}
			<div className="flex flex-wrap gap-2">
				<Button onClick={() => call("open_custom_tool", { id: tool.id })}>Open</Button>
				{(tool.kind === "web" || tool.homepage) && (
					<Button variant="outline" onClick={() => call("open_custom_homepage", { id: tool.id })}>
						{tool.kind === "web" ? "Open in browser" : "Where to get it"}
					</Button>
				)}
				<Button variant="outline" onClick={onEdit}>
					Edit
				</Button>
				<Button
					variant="destructive"
					onClick={() =>
						confirmRemove === tool.id ? call("remove_custom_tool", { id: tool.id }) : setConfirmRemove(tool.id)
					}
				>
					{confirmRemove === tool.id ? "Click again to remove" : "Remove"}
				</Button>
			</div>
		</>
	);
}

function SuggestionCard({
	suggestion,
	call,
	onAddProgram,
}: {
	suggestion: Suggestion;
	call: Call;
	onAddProgram: () => void;
}) {
	return (
		<Card size="sm">
			<CardContent className="flex gap-3">
				<ToolImage src={suggestion.icon} kind={suggestion.kind} />
				<div className="flex min-w-0 flex-col">
					<span className="font-medium">{suggestion.name}</span>
					<span className="text-xs text-muted-foreground">
						{suggestion.kind === "web" ? "Web page" : "Program"}
						{suggestion.author && ` by ${suggestion.author}`}
					</span>
					{suggestion.description && (
						<span className="text-xs text-muted-foreground">{suggestion.description}</span>
					)}
				</div>
			</CardContent>
			<CardFooter className="gap-2">
				{suggestion.added ? (
					<Badge variant="secondary">Added</Badge>
				) : suggestion.kind === "web" ? (
					<Button onClick={() => call("add_suggested_tool", { id: suggestion.id })}>Add</Button>
				) : (
					<>
						<Button variant="outline" onClick={() => call("open_suggestion_page", { id: suggestion.id })}>
							Get it
						</Button>
						<Button onClick={onAddProgram}>Add...</Button>
					</>
				)}
			</CardFooter>
		</Card>
	);
}

export function App() {
	const [list, setList] = useState<ToolList | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [settings, setSettings] = useState<Settings | null>(null);
	const [selected, setSelected] = useState<Selected | null>(null);
	// The tool whose Uninstall or Remove was clicked once and waits for a second click.
	const [confirm, setConfirm] = useState<string | null>(null);
	// The custom tool dialog: closed, adding (null) or editing a tool.
	const [editing, setEditing] = useState<{ tool: CustomTool | null } | null>(null);

	const refresh = useCallback(() => {
		invoke<ToolList>("list_tools").then(setList, (e) => setError(String(e)));
	}, []);

	useEffect(() => {
		refresh();
		invoke<Settings>("get_settings").then(setSettings, (e) => setError(String(e)));
		const unlisten = listen("tools-changed", refresh);
		// Read again when the window comes back, in case tool folders changed meanwhile.
		window.addEventListener("focus", refresh);
		return () => {
			unlisten.then((stop) => stop());
			window.removeEventListener("focus", refresh);
		};
	}, [refresh]);

	// A failed start or install shows with its tool (the list keeps it), so only other failures
	// show here.
	const call: Call = (cmd, args = {}) => {
		setError(null);
		setConfirm(null);
		invoke(cmd, args).catch((e) => {
			if (cmd !== "start_tool" || args.project) setError(String(e));
		});
	};

	const addProgram = async (suggestion: Suggestion) => {
		setError(null);
		try {
			const program = await invoke<string | null>("pick_program");
			if (program) await invoke("add_suggested_tool", { id: suggestion.id, program });
		} catch (e) {
			setError(String(e));
		}
	};

	const launcher = list?.launcher;
	const anyRunning = list?.tools.some((t) => t.state !== "stopped") ?? false;
	const anyBusy = list?.tools.some((t) => t.task || t.updateWaiting) ?? false;
	const checking = list?.indexChecking || launcher?.checking;

	const selectedTool = selected?.type === "tool" ? list?.tools.find((t) => t.id === selected.id) : undefined;
	const selectedCustom =
		selected?.type === "custom" ? list?.customTools.find((t) => t.id === selected.id) : undefined;
	const select = (s: Selected) => {
		setConfirm(null);
		setSelected(s);
	};
	const installedTools = list?.tools.filter((t) => t.version !== null) ?? [];

	return (
		<main className="flex min-h-screen flex-col gap-6 p-6">
			<header className="flex flex-wrap items-center gap-3">
				<h1 className="font-heading text-2xl font-semibold">Crystal Tool Suite</h1>
				<span className="text-sm text-muted-foreground">Launcher {list?.launcherVersion}</span>
				<Button variant="outline" size="sm" disabled={checking} onClick={() => call("check_updates")}>
					{checking ? "Checking..." : "Check for updates"}
				</Button>
			</header>

			{(launcher?.available || launcher?.error || list?.indexError || error) && (
				<section className="flex flex-col gap-2">
					{launcher?.available && (
						<div className="flex flex-col gap-2 rounded-lg border p-3">
							<span className="font-medium">Launcher {launcher.available.version} is available</span>
							{launcher.available.notes && (
								<pre className="whitespace-pre-wrap font-sans text-sm text-muted-foreground">
									{launcher.available.notes}
								</pre>
							)}
							<div className="flex items-center gap-3">
								<Button
									disabled={launcher.installing || anyRunning || anyBusy}
									onClick={() => call("update_launcher")}
								>
									{launcher.installing ? "Updating..." : "Update and restart"}
								</Button>
								{anyRunning && (
									<span className="text-sm text-muted-foreground">Close your tools to update the launcher.</span>
								)}
								{!anyRunning && anyBusy && (
									<span className="text-sm text-muted-foreground">Waiting for tool installs to finish.</span>
								)}
							</div>
						</div>
					)}
					{launcher?.error && <p className="text-sm text-destructive">{launcher.error}</p>}
					{list?.indexError && <p className="text-sm text-destructive">{list.indexError}</p>}
					{error && <p className="text-sm text-destructive">{error}</p>}
				</section>
			)}

			<div className="flex flex-col gap-6 md:flex-row md:items-start">
				<div className="flex min-w-0 flex-1 flex-col gap-6">
					<section className="flex flex-col gap-3">
						<h2 className="text-lg font-medium">Tools</h2>
						{list && list.tools.length === 0 && (
							<p className="text-sm text-muted-foreground">
								No tools yet. Tools from the suite index show here once it's read. For development, list tool
								folders in {list.devToolsFile} as a JSON array of paths.
							</p>
						)}
						<div className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
							{list?.tools.map((tool) => (
								<ToolCard
									key={tool.id}
									tool={tool}
									call={call}
									onDetails={() => select({ type: "tool", id: tool.id })}
								/>
							))}
						</div>
						{list?.problems.map((p) => (
							<p key={p.folder} className="text-sm text-muted-foreground">
								Skipped {p.folder}: {p.message}
							</p>
						))}
					</section>

					<section className="flex flex-col gap-3">
						<div className="flex items-center gap-3">
							<h2 className="text-lg font-medium">Custom tools</h2>
							<Button variant="outline" size="sm" onClick={() => setEditing({ tool: null })}>
								<Plus />
								Add a custom tool
							</Button>
						</div>
						{list?.customError && <p className="text-sm text-destructive">{list.customError}</p>}
						{list && list.customTools.length === 0 && (
							<p className="text-sm text-muted-foreground">
								Tools the suite doesn't ship: a web page, or a program on this computer.
							</p>
						)}
						<div className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
							{list?.customTools.map((tool) => (
								<CustomCard
									key={tool.id}
									tool={tool}
									call={call}
									onDetails={() => select({ type: "custom", id: tool.id })}
								/>
							))}
						</div>
						{list && list.suggestions.length > 0 && (
							<>
								<h3 className="text-sm font-medium text-muted-foreground">Suggested</h3>
								<div className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
									{list.suggestions.map((s) => (
										<SuggestionCard key={s.id} suggestion={s} call={call} onAddProgram={() => addProgram(s)} />
									))}
								</div>
							</>
						)}
					</section>

					<section className="flex flex-col gap-3">
						<h2 className="text-lg font-medium">Recent projects</h2>
						{list?.projectsError && <p className="text-sm text-destructive">{list.projectsError}</p>}
						{list && list.projects.length === 0 && !list.projectsError && (
							<p className="text-sm text-muted-foreground">
								None yet. The folders you choose in the suite's tools show here.
							</p>
						)}
						{list && list.projects.length > 0 && (
							<ul className="flex flex-col divide-y rounded-lg border">
								{list.projects.map((p) => {
									const tools = installedTools.filter((t) => t.folders.includes(p.kind));
									return (
										<li key={`${p.kind}|${p.path}`} className="flex flex-col gap-2 p-3">
											<div className="flex flex-wrap items-center gap-2">
												<span className="break-all text-sm font-medium">{p.path}</span>
												<Badge variant="outline">{p.kind}</Badge>
											</div>
											<span className="text-xs text-muted-foreground">
												Last used {when(p.lastUsed)}
												{p.tool && ` by ${list.tools.find((t) => t.id === p.tool)?.name ?? p.tool}`}
												{!p.exists && " · the folder isn't there any more"}
											</span>
											{p.exists && tools.length > 0 && (
												<div className="flex flex-wrap gap-2">
													{tools.map((t) => (
														<Button
															key={t.id}
															variant="outline"
															size="sm"
															disabled={t.state === "starting" || t.task !== null}
															onClick={() =>
																call("start_tool", { id: t.id, project: { path: p.path, kind: p.kind } })
															}
														>
															Open in {t.name}
														</Button>
													))}
												</div>
											)}
										</li>
									);
								})}
							</ul>
						)}
					</section>

					{settings && (
						<section className="flex flex-col gap-2">
							<h2 className="text-lg font-medium">Settings</h2>
							<label className="flex flex-col gap-1 text-sm">
								Suite index
								<input
									className="rounded-md border bg-background px-2 py-1"
									value={settings.indexUrl}
									onChange={(e) => setSettings({ ...settings, indexUrl: e.target.value })}
								/>
							</label>
							<label className="flex flex-col gap-1 text-sm">
								Launcher updates
								<input
									className="rounded-md border bg-background px-2 py-1"
									value={settings.launcherUpdateUrl}
									onChange={(e) => setSettings({ ...settings, launcherUpdateUrl: e.target.value })}
								/>
							</label>
							<div className="flex flex-wrap items-center gap-3">
								<Button variant="outline" size="sm" onClick={() => call("save_settings", { settings })}>
									Save and check
								</Button>
								<span className="text-sm text-muted-foreground">Saved in {list?.settingsFile}</span>
							</div>
						</section>
					)}
				</div>

				{list && (selectedTool || selectedCustom) && (
					<aside className="flex w-full flex-col gap-3 rounded-xl p-4 text-sm ring-1 ring-foreground/10 md:sticky md:top-6 md:w-80 md:shrink-0">
						<div className="flex items-center gap-3">
							<ToolImage
								src={selectedTool ? selectedTool.icon : (selectedCustom?.image ?? null)}
								kind={selectedCustom?.kind ?? "suite"}
							/>
							<h2 className="min-w-0 flex-1 text-lg font-medium">{selectedTool?.name ?? selectedCustom?.name}</h2>
							<Button variant="ghost" size="icon-sm" aria-label="Close details" onClick={() => setSelected(null)}>
								<X />
							</Button>
						</div>
						{selectedTool && (
							<ToolDetails
								tool={selectedTool}
								list={list}
								call={call}
								confirmUninstall={confirm}
								setConfirmUninstall={setConfirm}
							/>
						)}
						{selectedCustom && (
							<CustomDetails
								tool={selectedCustom}
								call={call}
								onEdit={() => setEditing({ tool: selectedCustom })}
								confirmRemove={confirm}
								setConfirmRemove={setConfirm}
							/>
						)}
					</aside>
				)}
			</div>

			<CustomToolDialog open={editing !== null} tool={editing?.tool ?? null} onClose={() => setEditing(null)} />
		</main>
	);
}
