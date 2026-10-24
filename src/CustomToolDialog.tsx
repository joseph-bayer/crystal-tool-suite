import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ToolImage } from "@/ToolImage";
import type { CustomKind, CustomTool } from "@/types";

type Thumbnail = { action: "keep" } | { action: "remove" } | { action: "set"; path: string };

const inputClass = "rounded-md border bg-background px-2 py-1";

/** Adding a custom tool (`tool` null) or changing one. */
export function CustomToolDialog({
	open,
	tool,
	onClose,
}: {
	open: boolean;
	tool: CustomTool | null;
	onClose: () => void;
}) {
	const [name, setName] = useState("");
	const [kind, setKind] = useState<CustomKind>("web");
	const [url, setUrl] = useState("");
	const [program, setProgram] = useState("");
	const [args, setArgs] = useState("");
	const [thumbnail, setThumbnail] = useState<Thumbnail>({ action: "keep" });
	const [preview, setPreview] = useState<string | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [saving, setSaving] = useState(false);

	useEffect(() => {
		if (!open) return;
		setName(tool?.name ?? "");
		setKind(tool?.kind ?? "web");
		setUrl(tool?.url ?? "");
		setProgram(tool?.program ?? "");
		setArgs(tool?.args ?? "");
		setThumbnail({ action: "keep" });
		setPreview(tool?.image ?? null);
		setError(null);
		setSaving(false);
	}, [open, tool]);

	const pickProgram = async () => {
		setError(null);
		try {
			const path = await invoke<string | null>("pick_program");
			if (path) setProgram(path);
		} catch (e) {
			setError(String(e));
		}
	};

	const pickImage = async () => {
		setError(null);
		try {
			const picked = await invoke<{ path: string; dataUrl: string } | null>("pick_image");
			if (picked) {
				setThumbnail({ action: "set", path: picked.path });
				setPreview(picked.dataUrl);
			}
		} catch (e) {
			setError(String(e));
		}
	};

	const save = async () => {
		setError(null);
		setSaving(true);
		try {
			await invoke("save_custom_tool", {
				tool: { id: tool?.id ?? null, name, kind, url, program, args, thumbnail },
			});
			onClose();
		} catch (e) {
			setError(String(e));
			setSaving(false);
		}
	};

	return (
		<Dialog open={open} onOpenChange={(o) => !o && onClose()}>
			<DialogContent className="sm:max-w-lg">
				<DialogHeader>
					<DialogTitle>{tool ? `Edit ${tool.name}` : "Add a custom tool"}</DialogTitle>
					<DialogDescription>
						A web page opens in its own launcher window. A program on this computer is started as it is, and runs on
						its own.
					</DialogDescription>
				</DialogHeader>

				<label className="flex flex-col gap-1">
					Name
					<input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} />
				</label>

				<fieldset className="flex flex-wrap gap-4">
					<legend className="mb-1">It is</legend>
					<label className="flex items-center gap-2">
						<input type="radio" name="kind" checked={kind === "web"} onChange={() => setKind("web")} />
						A web page
					</label>
					<label className="flex items-center gap-2">
						<input type="radio" name="kind" checked={kind === "program"} onChange={() => setKind("program")} />
						A program on this computer
					</label>
				</fieldset>

				{kind === "web" ? (
					<label className="flex flex-col gap-1">
						Web address
						<input
							className={inputClass}
							placeholder="https://"
							value={url}
							onChange={(e) => setUrl(e.target.value)}
						/>
					</label>
				) : (
					<>
						<div className="flex flex-col gap-1">
							<label htmlFor="custom-program">Program</label>
							<div className="flex gap-2">
								<input
									id="custom-program"
									className={`${inputClass} min-w-0 flex-1`}
									value={program}
									onChange={(e) => setProgram(e.target.value)}
								/>
								<Button variant="outline" onClick={pickProgram}>
									Choose...
								</Button>
							</div>
						</div>
						<label className="flex flex-col gap-1">
							Arguments (optional)
							<input className={inputClass} value={args} onChange={(e) => setArgs(e.target.value)} />
						</label>
					</>
				)}

				<div className="flex flex-col gap-1">
					<span>Thumbnail</span>
					<div className="flex items-center gap-3">
						<ToolImage src={preview} kind={kind} />
						<Button variant="outline" onClick={pickImage}>
							Choose image...
						</Button>
						{preview && (
							<Button
								variant="ghost"
								onClick={() => {
									setThumbnail({ action: "remove" });
									setPreview(null);
								}}
							>
								Remove image
							</Button>
						)}
					</div>
				</div>

				{error && <p className="text-destructive">{error}</p>}

				<DialogFooter>
					<Button variant="outline" onClick={onClose}>
						Cancel
					</Button>
					<Button disabled={saving} onClick={save}>
						{tool ? "Save" : "Add"}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
