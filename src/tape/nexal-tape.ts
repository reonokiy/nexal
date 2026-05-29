import { Tape as BaseTape } from "@nexal/tape";
import type { TapeEntry, TapeFactoryOptions, TapeHandle, TapeStore } from "@nexal/tape";

export interface NexalSessionContext {
	channel: string;
	chatId: string;
	sessionKey: string;
	streaming?: boolean;
	debounce?: unknown;
}

export interface NexalWorkerContext {
	id: string;
	name: string;
	kind: string;
	lifetime: string;
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo?: string | null;
	initialPrompt?: string | null;
	sendPolicy: string;
	status: string;
	containerName?: string | null;
	sandboxKey?: string;
	sandboxed?: boolean;
	resumed?: boolean;
}

/** Nexal-specific semantics layered on top of the generic tape package. */
export class NexalTape extends BaseTape {
	static async create(store: TapeStore, maxContext?: number): Promise<NexalTape>;
	static async create(store: TapeStore, options?: TapeFactoryOptions): Promise<NexalTape>;
	static async create(store: TapeStore, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<NexalTape> {
		return new NexalTape({ store, ref: await store.create(), ...factoryOptions(optionsOrMaxContext) });
	}

	static async load(store: TapeStore, ref: TapeHandle, maxContext?: number): Promise<NexalTape>;
	static async load(store: TapeStore, ref: TapeHandle, options?: TapeFactoryOptions): Promise<NexalTape>;
	static async load(store: TapeStore, ref: TapeHandle, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<NexalTape> {
		const tape = new NexalTape({ store, ref, ...factoryOptions(optionsOrMaxContext) });
		await tape.entries();
		return tape;
	}

	static async loadOrCreate(store: TapeStore, ref: TapeHandle, maxContext?: number): Promise<NexalTape>;
	static async loadOrCreate(store: TapeStore, ref: TapeHandle, options?: TapeFactoryOptions): Promise<NexalTape>;
	static async loadOrCreate(store: TapeStore, ref: TapeHandle, optionsOrMaxContext?: number | TapeFactoryOptions): Promise<NexalTape> {
		return new NexalTape({ store, ref, ...factoryOptions(optionsOrMaxContext) });
	}

	async setSessionContext(context: NexalSessionContext): Promise<Array<TapeEntry | null>> {
		const metadata = {
			channel: context.channel,
			chatId: context.chatId,
			sessionKey: context.sessionKey,
		};
		const entries: Array<TapeEntry | null> = [];
		entries.push(await this.setContext(metadata, {
			scope: "session",
			ifChanged: true,
		}));
		entries.push(await this.setPolicy({
			streaming: context.streaming ?? false,
			debounce: context.debounce ?? null,
		}, {
			scope: "session",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setRuntime({
			type: "llm-session",
		}, {
			scope: "session",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setStatus("running", {
			scope: "session",
			metadata,
			ifChanged: true,
		}));
		return entries;
	}

	async setWorkerContext(context: NexalWorkerContext): Promise<Array<TapeEntry | null>> {
		const metadata = {
			id: context.id,
			name: context.name,
			kind: context.kind,
			lifetime: context.lifetime,
			parentSessionKey: context.parentSessionKey,
			sourceChannel: context.sourceChannel,
			sourceChatId: context.sourceChatId,
			sourceReplyTo: context.sourceReplyTo,
			sendPolicy: context.sendPolicy,
			initialPrompt: context.initialPrompt,
			containerName: context.containerName,
		};
		const entries: Array<TapeEntry | null> = [];
		entries.push(await this.setContext({
			id: context.id,
			name: context.name,
			kind: context.kind,
			parentSessionKey: context.parentSessionKey,
			sourceChannel: context.sourceChannel,
			sourceChatId: context.sourceChatId,
			sourceReplyTo: context.sourceReplyTo,
			initialPrompt: context.initialPrompt,
		}, {
			scope: "worker",
			ifChanged: true,
		}));
		entries.push(await this.setPolicy({
			lifetime: context.lifetime,
			sendPolicy: context.sendPolicy,
		}, {
			scope: "worker",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setRuntime({
			type: "agent-runner",
			sandboxKey: context.sandboxKey,
			containerName: context.containerName,
			sandboxed: context.sandboxed ?? context.kind === "executor",
			resumed: context.resumed ?? false,
		}, {
			scope: "worker",
			metadata,
			ifChanged: true,
		}));
		entries.push(await this.setStatus(context.status, {
			scope: "worker",
			metadata,
			ifChanged: true,
		}));
		return entries;
	}
}

function factoryOptions(optionsOrMaxContext?: number | TapeFactoryOptions): TapeFactoryOptions {
	return typeof optionsOrMaxContext === "number"
		? { maxContext: optionsOrMaxContext }
		: optionsOrMaxContext ?? {};
}
