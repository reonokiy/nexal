import type { OutgoingReply } from "../channels/types.ts";

/**
 * Abstract message sending — decouples consumers from Channel Map.
 * The implementation in index.ts delegates to the actual channel registry.
 */
export interface MessageSender {
    /** Send a reply to a specific channel + chat. */
    send(channelName: string, reply: OutgoingReply): Promise<void>;
    
    /** Send a streaming chunk (optional — not all channels support it). */
    sendChunk?(channelName: string, chatId: string, messageId: string, delta: string): void;
    
    /** Signal end of stream (optional). */
    sendEnd?(channelName: string, chatId: string, messageId: string): void;
}

/** Create a MessageSender backed by a channel Map. */
export function createMessageSender(
    channels: Map<string, { send(r: OutgoingReply): Promise<void>; sendChunk?(...a: any[]): void; sendEnd?(...a: any[]): void }>,
): MessageSender {
    return {
        async send(channelName, reply) {
            await channels.get(channelName)?.send(reply);
        },
        sendChunk(channelName, chatId, messageId, delta) {
            channels.get(channelName)?.sendChunk?.(chatId, messageId, delta);
        },
        sendEnd(channelName, chatId, messageId) {
            channels.get(channelName)?.sendEnd?.(chatId, messageId);
        },
    };
}
