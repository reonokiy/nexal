/**
 * Wire envelope — the minimal framing shared by every message.
 *
 * Three discriminated message kinds:
 *   Request      — has `id` + `method` + `params`, expects a Response
 *   Response     — has `id` + (`result` | `error`), matches a Request
 *   Notification — has `method` + `params` only, fire-and-forget
 */

export type MessageId = string | number;

export interface WireRequest {
	/** Virtual stream id for multiplexing. Omitted = connection-level. */
	stream?: string;
	id: MessageId;
	method: string;
	params?: unknown;
}

export interface WireResponse {
	stream?: string;
	id: MessageId;
	result?: unknown;
	error?: WireError;
}

export interface WireError {
	code: number;
	message: string;
	data?: unknown;
}

export interface WireNotification {
	stream?: string;
	method: string;
	params?: unknown;
}

export type WireMessage = WireRequest | WireResponse | WireNotification;

export function isWireRequest(msg: WireMessage): msg is WireRequest {
	return "id" in msg && "method" in msg;
}

export function isWireResponse(msg: WireMessage): msg is WireResponse {
	return "id" in msg && ("result" in msg || "error" in msg);
}

export function isWireNotification(msg: WireMessage): msg is WireNotification {
	return "method" in msg && !("id" in msg);
}
