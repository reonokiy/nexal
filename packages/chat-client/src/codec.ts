/**
 * Msgpack binary frame codec for the nexal WebSocket chat protocol.
 *
 * Replaces JSON text frames with msgpack binary frames for better
 * performance and native binary support (images, etc.).
 *
 * Works in any runtime that supports Uint8Array (browser, Bun, Node).
 */
import { encode, decode } from "@msgpack/msgpack";

/**
 * Encode a frame object into a msgpack binary payload.
 * Returns a Uint8Array suitable for sending over a WebSocket binary frame.
 */
export function encodeFrame(frame: unknown): Uint8Array {
	return encode(frame);
}

/**
 * Decode a msgpack binary payload back into a frame object.
 * Accepts Uint8Array, ArrayBuffer, or Buffer.
 */
export function decodeFrame(data: unknown): unknown {
	if (data instanceof Uint8Array) {
		return decode(data);
	}
	if (data instanceof ArrayBuffer) {
		return decode(new Uint8Array(data));
	}
	if (typeof data === "string") {
		// Fallback: treat as JSON for backward compatibility during migration
		return JSON.parse(data);
	}
	// Buffer / TypedArray (Bun/Node)
	if (data && ArrayBuffer.isView(data)) {
		return decode(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
	}
	throw new Error(`Unsupported frame data type: ${typeof data}`);
}
