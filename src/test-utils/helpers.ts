import type { MessageSender } from "../messaging/sender.ts";
import { createMessageSender } from "../messaging/sender.ts";

export function createStubSender(): MessageSender {
	return createMessageSender(new Map());
}
