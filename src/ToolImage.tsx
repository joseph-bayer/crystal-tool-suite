import { AppWindow, Globe, Package } from "lucide-react";

import { cn } from "@/lib/utils";

/** A tool's icon or thumbnail, or a plain placeholder when it has none. */
export function ToolImage({
	src,
	kind = "suite",
	className,
}: {
	src: string | null;
	kind?: "suite" | "web" | "program";
	className?: string;
}) {
	const box = cn("size-10 shrink-0 rounded-md", className);
	if (src) return <img src={src} alt="" className={cn(box, "object-contain")} />;
	const Placeholder = kind === "web" ? Globe : kind === "program" ? AppWindow : Package;
	return (
		<div className={cn(box, "flex items-center justify-center bg-muted text-muted-foreground")}>
			<Placeholder className="size-5" />
		</div>
	);
}
