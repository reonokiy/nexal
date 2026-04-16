/**
 * CommandRegistry — shared slash-command registry.
 *
 * Channels parse commands according to their own protocol (Telegram bot
 * commands, WS/TUI text prefix, HTTP body field) and call
 * `registry.execute()` with a unified `CommandContext`.
 *
 * Commands are registered once at startup; the registry is passed to
 * every channel that wants to support slash commands.
 */

export interface CommandContext {
	/** Channel name, e.g. "telegram", "ws", "http". */
	channel: string;
	/** Chat/session ID on that channel. */
	chatId: string;
	/** Human-readable sender name. */
	sender: string;
}

export interface CommandResult {
	/** Reply text (markdown OK — channels render as they see fit). */
	text: string;
}

export interface CommandDef {
	/** Command name without leading slash, e.g. "help". */
	name: string;
	/** Short description shown in /help and Telegram command menu. */
	description: string;
	/** Execute the command. */
	execute: (ctx: CommandContext, args: string[]) => Promise<CommandResult>;
}

export class CommandRegistry {
	private readonly commands = new Map<string, CommandDef>();

	register(def: CommandDef): void {
		this.commands.set(def.name, def);
	}

	async execute(
		name: string,
		ctx: CommandContext,
		args: string[],
	): Promise<CommandResult | null> {
		const def = this.commands.get(name);
		if (!def) return null;
		return def.execute(ctx, args);
	}

	/** All registered commands (for /help listing and Telegram setMyCommands). */
	list(): CommandDef[] {
		return [...this.commands.values()];
	}

	has(name: string): boolean {
		return this.commands.has(name);
	}
}
