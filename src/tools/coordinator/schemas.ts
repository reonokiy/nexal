import { Type } from "@mariozechner/pi-ai";

import { UserContentSchema } from "../../content.ts";

export interface CoordinatorCtx {
	/**
	 * Identifier used as the `parent_session_key` of agents spawned
	 * here. For the top-level coordinator it's `"<channel>:<chatId>"`;
	 * for a sub-coordinator it's the sub-coordinator's own row id.
	 */
	parentSessionKey: string;
	sourceChannel: string;
	sourceChatId: string;
	sourceReplyTo?: string | null;
}

export const SpawnExecutorParams = Type.Object({
	name: Type.String({
		description:
			"Short kebab-case label (e.g. \"refactor-authz\"). Shown as a prefix on " +
			"every chat message the executor emits via send_to_user.",
	}),
	system_prompt: Type.String({
		description:
			"The executor's persona / capability description. Frozen at spawn time. " +
			"Be specific about role, allowed actions, and reporting style. Tell it to " +
			"call send_to_user for milestones.",
	}),
	initial_prompt: Type.Optional(
		Type.String({
			description:
				"Optional first user message. Omit to spawn an empty executor that waits " +
				"for a send_to_agent call.",
		}),
	),
	send_policy: Type.Optional(
		Type.Union([Type.Literal("explicit"), Type.Literal("final"), Type.Literal("all")], {
			description:
				"explicit (default) = only send_to_user reaches chat; final = + last assistant text per turn; all = every assistant turn.",
		}),
	),
});

export const SpawnOneshotParams = Type.Object({
	name: Type.String({ description: "Short kebab-case label (chat-message prefix)." }),
	prompt: Type.String({
		description: "Full instructions. The executor runs once and dies on completion.",
	}),
	system_prompt: Type.Optional(
		Type.String({ description: "Optional override of the default executor system prompt." }),
	),
	send_policy: Type.Optional(
		Type.Union([Type.Literal("explicit"), Type.Literal("final"), Type.Literal("all")], {
			description: "Default explicit; pick final to auto-send the executor's last reply.",
		}),
	),
});

export const SpawnCoordinatorParams = Type.Object({
	name: Type.String({
		description:
			"Short kebab-case label for the sub-coordinator. Shown if it ever sends " +
			"directly to the user (rare — coordinators normally route, not talk).",
	}),
	system_prompt: Type.String({
		description:
			"The sub-coordinator's identity: what domain it owns, when to spawn vs route, " +
			"what kinds of executors live under it. Frozen at spawn time.",
	}),
	initial_prompt: Type.Optional(
		Type.String({
			description:
				"Optional first user message. Often omitted — sub-coordinators usually start " +
				"idle and wait for the parent to route work to them.",
		}),
	),
});

export const RouteParams = Type.Object({
	id: Type.String({ description: "Agent id from spawn_* / list_agents." }),
	content: UserContentSchema,
});

export const IdParams = Type.Object({
	id: Type.String({ description: "Agent id." }),
});

export const ListParams = Type.Object({});
