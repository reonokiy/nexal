/**
 * Binary frame codec — msgpack encode/decode.
 *
 * Every wire frame is a single msgpack-encoded binary payload.
 * Transport delivers raw `Uint8Array` frames; this module converts
 * between application objects and those frames.
 */
import { encode, decode } from "@msgpack/msgpack";

/** Encode an object into a msgpack binary frame. */
export function encodeFrame(frame: unknown): Uint8Array {
	return encode(frame);
}

/** Decode a msgpack binary frame back into an object. */
export function decodeFrame<T = unknown>(data: Uint8Array): T {
	return decode(data) as T;
}
