import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri loads the dev server from a fixed port (src-tauri/tauri.conf.json's devUrl).
export default defineConfig({
	plugins: [react(), tailwindcss()],
	resolve: {
		alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
	},
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		watch: { ignored: ["**/src-tauri/**"] },
	},
	build: {
		outDir: "dist",
		emptyOutDir: true,
	},
});
