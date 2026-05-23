import type { FileRef } from "./types.ts";

export interface FileStore {
	/**
	 * Upload bytes to external storage and return a FileRef.
	 * If the same content already exists (hash collision) the upload
	 * is skipped and the existing ref is returned immediately.
	 */
	upload(
		data: Uint8Array | Buffer,
		mimeType: string,
		filename: string,
	): Promise<FileRef>;
	/** Download bytes by content hash, or null if not found. */
	download(fileHash: string): Promise<Uint8Array | null>;
	/** Get a presigned (or public) URL for a file by its content hash. */
	getUrl(fileHash: string): Promise<string | null>;
	/** Close any open connections. */
	close?(): Promise<void>;
}
