/**
 * FileStore — content-addressed binary storage backed by Bun.S3Client
 * (zero dependencies; uses Bun’s built-in S3 support).
 *
 * Deduplication: files are keyed by SHA-256(content). Same bytes → same
 * storage path, single physical copy.
 */
import { createHash } from "node:crypto";
import type { FileRef } from "./types.ts";
import type { StorageConfig } from "../config.ts";

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

// ── S3FileStore (Supabase / any S3-compatible) ───────────────────────

export class S3FileStore implements FileStore {
	private readonly client: any; // Bun.S3Client
	private readonly bucket: string;

	constructor(cfg: StorageConfig) {
		if (cfg.provider !== "s3") {
			throw new Error("S3FileStore requires provider: s3");
		}
		this.bucket = cfg.s3Bucket;
		// Bun.S3Client is built-in since Bun 1.2
		this.client = new (Bun as any).S3Client({
			accessKeyId: cfg.s3AccessKey,
			secretAccessKey: cfg.s3SecretKey,
			endpoint: cfg.s3Endpoint,
			bucket: cfg.s3Bucket,
			region: cfg.s3Region,
		});
	}

	async upload(
		data: Uint8Array | Buffer,
		mimeType: string,
		filename: string,
	): Promise<FileRef> {
		const hash = sha256Hex(data);
		const file = this.client.file(hashPath(hash));
		// PutObject is idempotent for the same key, so we always write.
		await file.write(data, { type: mimeType });
		const url = await file.presignedUrl({ expiresIn: 3600 });
		return {
			fileHash: hash,
			mimeType,
			filename,
			sizeBytes: data.byteLength,
			url,
		};
	}

	async download(fileHash: string): Promise<Uint8Array | null> {
		try {
			const file = this.client.file(hashPath(fileHash));
			const buf = await file.arrayBuffer();
			return new Uint8Array(buf);
		} catch {
			return null;
		}
	}

	async getUrl(fileHash: string): Promise<string | null> {
		try {
			const file = this.client.file(hashPath(fileHash));
			return await file.presignedUrl({ expiresIn: 3600 });
		} catch {
			return null;
		}
	}
}

// ── LocalFileStore (for dev / tests) ─────────────────────────────────

export class LocalFileStore implements FileStore {
	private readonly root: string;

	constructor(cfg: StorageConfig) {
		if (cfg.provider !== "local") {
			throw new Error("LocalFileStore requires provider: local");
		}
		this.root = cfg.s3Endpoint || "./tape-files";
	}

	async upload(
		data: Uint8Array | Buffer,
		mimeType: string,
		filename: string,
	): Promise<FileRef> {
		const hash = sha256Hex(data);
		const path = hashPath(hash);
		const fullPath = `${this.root}/${path}`;
		await Bun.write(fullPath, data);
		return {
			fileHash: hash,
			mimeType,
			filename,
			sizeBytes: data.byteLength,
			url: `file://${fullPath}`,
		};
	}

	async download(fileHash: string): Promise<Uint8Array | null> {
		const fullPath = `${this.root}/${hashPath(fileHash)}`;
		const file = Bun.file(fullPath);
		if (!(await file.exists())) return null;
		return new Uint8Array(await file.arrayBuffer());
	}

	async getUrl(fileHash: string): Promise<string | null> {
		const fullPath = `${this.root}/${hashPath(fileHash)}`;
		const exists = await Bun.file(fullPath).exists();
		return exists ? `file://${fullPath}` : null;
	}
}

// ── helpers ──────────────────────────────────────────────────────────

function sha256Hex(data: Uint8Array | Buffer): string {
	return createHash("sha256").update(data).digest("hex");
}

/** Git-object-style directory sharding to avoid single-dir explosion. */
function hashPath(hash: string): string {
	return `${hash.slice(0, 2)}/${hash.slice(2, 4)}/${hash}`;
}

/** Factory — picks the right implementation from config. */
export function createFileStore(cfg: StorageConfig): FileStore {
	switch (cfg.provider) {
		case "s3":
			return new S3FileStore(cfg);
		case "local":
			return new LocalFileStore(cfg);
		default:
			throw new Error(`Unknown storage provider: ${(cfg as any).provider}`);
	}
}
