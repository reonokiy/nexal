export interface ImageAttachment {
	/** Raw image bytes (Uint8Array) or base64 data URL. */
	data: Uint8Array | string;
	/** MIME type, e.g. "image/jpeg". */
	mimeType: string;
	/** Original filename (best-effort). */
	filename: string;
}
