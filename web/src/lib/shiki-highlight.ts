import {
	createHighlighterCore,
	type HighlighterCore,
	type LanguageRegistration,
} from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import githubDark from "shiki/themes/github-dark.mjs";
import githubLight from "shiki/themes/github-light.mjs";
import bash from "shiki/langs/bash.mjs";
import css from "shiki/langs/css.mjs";
import diff from "shiki/langs/diff.mjs";
import dockerfile from "shiki/langs/dockerfile.mjs";
import dotenv from "shiki/langs/dotenv.mjs";
import go from "shiki/langs/go.mjs";
import html from "shiki/langs/html.mjs";
import ini from "shiki/langs/ini.mjs";
import javascript from "shiki/langs/javascript.mjs";
import json from "shiki/langs/json.mjs";
import jsonc from "shiki/langs/jsonc.mjs";
import jsx from "shiki/langs/jsx.mjs";
import markdown from "shiki/langs/markdown.mjs";
import nginx from "shiki/langs/nginx.mjs";
import python from "shiki/langs/python.mjs";
import rust from "shiki/langs/rust.mjs";
import sql from "shiki/langs/sql.mjs";
import svelte from "shiki/langs/svelte.mjs";
import toml from "shiki/langs/toml.mjs";
import tsx from "shiki/langs/tsx.mjs";
import typescript from "shiki/langs/typescript.mjs";
import vue from "shiki/langs/vue.mjs";
import xml from "shiki/langs/xml.mjs";
import yaml from "shiki/langs/yaml.mjs";

const supportedLanguages = new Set([
	"bash",
	"css",
	"diff",
	"dockerfile",
	"dotenv",
	"go",
	"html",
	"ini",
	"javascript",
	"json",
	"jsonc",
	"jsx",
	"markdown",
	"nginx",
	"python",
	"rust",
	"sql",
	"svelte",
	"toml",
	"tsx",
	"typescript",
	"vue",
	"xml",
	"yaml",
]);

let highlighterPromise: Promise<HighlighterCore> | null = null;

export function resolveShikiLanguage(value: string): string | null {
	const raw = value.trim().split(/\s+/)[0]?.toLowerCase().replace(/^\./, "");
	if (!raw) return null;

	const aliases: Record<string, string> = {
		caddyfile: "nginx",
		console: "bash",
		docker: "dockerfile",
		env: "dotenv",
		js: "javascript",
		md: "markdown",
		plaintext: "",
		plain: "",
		rs: "rust",
		sh: "bash",
		shell: "bash",
		text: "",
		ts: "typescript",
		txt: "",
		yml: "yaml",
	};
	const candidate = aliases[raw] ?? raw;
	if (!candidate) return null;
	return supportedLanguages.has(candidate) ? candidate : null;
}

export async function highlightCode(code: string, language: string): Promise<string> {
	const highlighter = await getHighlighter();
	return highlighter.codeToHtml(code, {
		lang: language,
		themes: {
			light: "github-light",
			dark: "github-dark",
		},
		defaultColor: false,
	});
}

function getHighlighter(): Promise<HighlighterCore> {
	highlighterPromise ??= createHighlighterCore({
		themes: [githubLight, githubDark],
		langs: [
			...bash,
			...css,
			...diff,
			...dockerfile,
			...dotenv,
			...go,
			...html,
			...ini,
			...javascript,
			...json,
			...jsonc,
			...jsx,
			...markdown,
			...nginx,
			...python,
			...rust,
			...sql,
			...svelte,
			...toml,
			...tsx,
			...typescript,
			...vue,
			...xml,
			...yaml,
		] satisfies LanguageRegistration[],
		engine: createJavaScriptRegexEngine(),
	});
	return highlighterPromise;
}
