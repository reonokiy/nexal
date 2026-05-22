import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

const isGithubPages = process.env.GITHUB_PAGES === "true";
const pagesBasePath = process.env.PAGES_BASE_PATH;
const repoName = process.env.GITHUB_REPOSITORY?.split("/")[1] ?? "nexal";

export default defineConfig({
	base: isGithubPages ? (pagesBasePath || `/${repoName}/`) : "/",
	plugins: [svelte(), tailwindcss()],
	resolve: {
		alias: {
			$lib: fileURLToPath(new URL("./src/lib", import.meta.url)),
		},
	},
	server: {
		host: true,
		port: 5173,
	},
});
