import type { Channel } from "./types.ts";
import type { CommandRegistry } from "../commands/registry.ts";
import type { GatewayClient } from "../gateway/index.ts";

export interface ChannelFactoryContext {
	cfg: Record<string, unknown>;
	commands?: CommandRegistry;
	gateway?: GatewayClient;
}

export type ChannelFactory = (ctx: ChannelFactoryContext) => Channel | null;

const factories = new Map<string, ChannelFactory>();

export function registerChannel(name: string, factory: ChannelFactory): void {
	factories.set(name, factory);
}

export function buildRegisteredChannel(
	name: string,
	cfg: Record<string, unknown>,
	commands?: CommandRegistry,
	gateway?: GatewayClient,
): Channel | null {
	const factory = factories.get(name);
	return factory ? factory({ cfg, commands, gateway }) : null;
}

export function getRegisteredChannels(): string[] {
	return [...factories.keys()];
}
