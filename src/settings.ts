/**
 * Settings store — simple KV backed by the shared Postgres connection
 * (`src/db.ts`). The `settings` table is created by drizzle migrations.
 *
 * Persists:
 *   - API keys per provider           (auth:<provider>)
 *   - Model provider / model ID prefs (model:provider, model:id)
 *   - Channel config buckets          (channel:<name>)
 *
 * (Previously PGlite-backed; PGlite was removed in favour of a single
 * external Postgres shared with the worker store.)
 */
import { eq, like } from "drizzle-orm";

import { getDb } from "./db.ts";
import { settings } from "./schema.ts";

export async function getSetting(key: string): Promise<string | null> {
	const rows = await getDb()
		.select({ value: settings.value })
		.from(settings)
		.where(eq(settings.key, key));
	return rows[0]?.value ?? null;
}

export async function setSetting(key: string, value: string): Promise<void> {
	await getDb()
		.insert(settings)
		.values({ key, value })
		.onConflictDoUpdate({ target: settings.key, set: { value } });
}

export async function deleteSetting(key: string): Promise<void> {
	await getDb().delete(settings).where(eq(settings.key, key));
}

// ── Auth helpers ────────────────────────────────────────────────────

export interface SavedAuth {
	provider: string;
	apiKey: string;
}

export async function saveAuth(auth: SavedAuth): Promise<void> {
	await setSetting(`auth:${auth.provider}`, JSON.stringify(auth));
}

export async function loadAuth(provider: string): Promise<SavedAuth | null> {
	const raw = await getSetting(`auth:${provider}`);
	if (!raw) return null;
	const parsed = JSON.parse(raw) as Partial<SavedAuth> & { apiKey?: string };
	if (!parsed.apiKey) return null;
	return { provider, apiKey: parsed.apiKey };
}

export async function deleteAuth(provider: string): Promise<void> {
	await deleteSetting(`auth:${provider}`);
}

export async function saveModelConfig(provider: string, modelId: string): Promise<void> {
	await setSetting("model:provider", provider);
	await setSetting("model:id", modelId);
}

export async function loadModelConfig(): Promise<{ provider: string; modelId: string } | null> {
	const provider = await getSetting("model:provider");
	const modelId = await getSetting("model:id");
	if (!provider || !modelId) return null;
	return { provider, modelId };
}

// ── Channel config helpers ──────────────────────────────────────────
//
// Channel configuration lives in the DB only (TOML/env `[channel.*]` is
// deprecated). Same JSON-blob-in-KV pattern as auth: key
// `channel:<name>` → the channel's config bucket. Writers fire
// `notifyChannelConfigChanged` so the ChannelManager can hot-reload
// without a poll round-trip.

type ChannelConfigBucket = Record<string, unknown>;

const channelConfigListeners = new Set<() => void>();

/** Subscribe to channel-config writes. Returns an unsubscribe fn. */
export function onChannelConfigChange(fn: () => void): () => void {
	channelConfigListeners.add(fn);
	return () => channelConfigListeners.delete(fn);
}

function notifyChannelConfigChanged(): void {
	for (const fn of channelConfigListeners) {
		try {
			fn();
		} catch {
			// A misbehaving listener must not break the writer.
		}
	}
}

export async function saveChannelConfig(name: string, config: ChannelConfigBucket): Promise<void> {
	await setSetting(`channel:${name}`, JSON.stringify(config));
	notifyChannelConfigChanged();
}

export async function loadChannelConfig(name: string): Promise<ChannelConfigBucket | null> {
	const raw = await getSetting(`channel:${name}`);
	if (!raw) return null;
	return JSON.parse(raw) as ChannelConfigBucket;
}

export async function deleteChannelConfig(name: string): Promise<void> {
	await deleteSetting(`channel:${name}`);
	notifyChannelConfigChanged();
}

export async function loadAllChannelConfigs(): Promise<Record<string, ChannelConfigBucket>> {
	const rows = await getDb()
		.select()
		.from(settings)
		.where(like(settings.key, "channel:%"));
	const out: Record<string, ChannelConfigBucket> = {};
	for (const row of rows) {
		const name = row.key.slice("channel:".length);
		try {
			out[name] = JSON.parse(row.value) as ChannelConfigBucket;
		} catch {
			// Skip a corrupt row rather than crash the whole reconcile.
		}
	}
	return out;
}

// ── Provider config helpers ──────────────────────────────────────────
//
// Provider config lives in the DB under `provider:<name>` keys. Each
// holds a JSON blob with baseUrl, wireApi, thinkingMode, etc.
// Loaded on startup and merged into `cfg.providers`.

type ProviderConfigBucket = Record<string, unknown>;

export async function saveProviderConfig(name: string, config: ProviderConfigBucket): Promise<void> {
	await setSetting(`provider:${name}`, JSON.stringify(config));
}

export async function loadProviderConfig(name: string): Promise<ProviderConfigBucket | null> {
	const raw = await getSetting(`provider:${name}`);
	if (!raw) return null;
	return JSON.parse(raw) as ProviderConfigBucket;
}

export async function loadAllProviderConfigs(): Promise<Record<string, ProviderConfigBucket>> {
	const rows = await getDb()
		.select()
		.from(settings)
		.where(like(settings.key, "provider:%"));
	const out: Record<string, ProviderConfigBucket> = {};
	for (const row of rows) {
		const name = row.key.slice("provider:".length);
		try {
			out[name] = JSON.parse(row.value) as ProviderConfigBucket;
		} catch {
			// skip corrupt
		}
	}
	return out;
}

// ── Tool API key helpers ────────────────────────────────────────────
//
// External tool keys (Tavily, Jina, Gemini, …) stored as
// `tool:apikey:<name>` → { name, apiKey }.

export async function saveToolApiKey(name: string, apiKey: string): Promise<void> {
	await setSetting(`tool:apikey:${name}`, JSON.stringify({ name, apiKey }));
}

export async function loadToolApiKey(name: string): Promise<string | null> {
	const raw = await getSetting(`tool:apikey:${name}`);
	if (!raw) return null;
	try {
		const { apiKey } = JSON.parse(raw);
		return typeof apiKey === "string" ? apiKey : null;
	} catch {
		return null;
	}
}

export async function loadAllToolApiKeys(): Promise<Record<string, string>> {
	const rows = await getDb()
		.select()
		.from(settings)
		.where(like(settings.key, "tool:apikey:%"));
	const out: Record<string, string> = {};
	for (const row of rows) {
		const name = row.key.slice("tool:apikey:".length);
		try {
			const { apiKey } = JSON.parse(row.value);
			if (typeof apiKey === "string") out[name] = apiKey;
		} catch {}
	}
	return out;
}
