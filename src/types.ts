// What the launcher's Rust side sends the page (see src-tauri/src/commands.rs).

export type Release = {
	version: string;
	notes: string | null;
	hasNotesUrl: boolean;
};

export type ToolRow = {
	id: string;
	name: string;
	description: string;
	version: string | null;
	folder: string | null;
	dev: boolean;
	diskUse: number | null;
	keptVersion: string | null;
	latest: Release | null;
	updateAvailable: boolean;
	rollBackTo: string | null;
	state: "stopped" | "starting" | "running";
	error: string | null;
	task: string | null;
	updateWaiting: boolean;
	installError: string | null;
	icon: string | null;
	folders: string[];
	canReport: boolean;
};

export type LauncherState = {
	configured: boolean;
	checking: boolean;
	installing: boolean;
	available: { version: string; notes: string | null } | null;
	error: string | null;
};

export type Project = {
	kind: string;
	path: string;
	lastUsed: string;
	tool: string;
	exists: boolean;
};

export type CustomKind = "web" | "program";

export type CustomTool = {
	id: string;
	name: string;
	kind: CustomKind;
	url: string | null;
	program: string | null;
	args: string | null;
	thumbnail: string | null;
	homepage: string | null;
	suggestion: string | null;
	/** The thumbnail as a data: URL. */
	image: string | null;
};

export type Suggestion = {
	id: string;
	name: string;
	author: string | null;
	description: string;
	kind: CustomKind;
	url: string | null;
	homepage: string | null;
	downloadUrl: string | null;
	icon: string | null;
	added: boolean;
};

export type ToolList = {
	tools: ToolRow[];
	problems: { folder: string; message: string }[];
	toolsDir: string;
	devToolsFile: string;
	settingsFile: string;
	indexError: string | null;
	indexChecking: boolean;
	launcherVersion: string;
	launcher: LauncherState;
	projects: Project[];
	projectsError: string | null;
	knownPathsFile: string;
	customTools: CustomTool[];
	customError: string | null;
	customToolsFile: string;
	suggestions: Suggestion[];
};

export type Settings = {
	indexUrl: string;
	launcherUpdateUrl: string;
};
