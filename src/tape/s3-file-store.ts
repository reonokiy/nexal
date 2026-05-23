/**
 * S3FileStore — content-addressed binary storage backed by Bun.S3Client
 * (uses Bun's built-in S3 support).
 *
 * Deduplication: files are keyed by SHA-256(content). Same bytes → same
 * storage path, single physical copy.
 */
import { createHash } from "node:crypto";
import type { FileRef, FileStore } from "@nexal/tape";
import type { StorageConfig } from "../config.ts";

export class S3FileStore implements FileStore {
	private readonly client: any; // Bun.S3Client
	private readonly bucket: string;

	constructor(cfg: StorageConfig) {
		if (cfg.provider !== "s3") {
			throw new Error("S3FileStore requires provider: s3");
		}
		this.bucket = cfg.s3Bucket;
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

// ── helpers ──────────────────────────────────────────────────────────

function sha256Hex(data: Uint8Array | Buffer): string {
	return createHash("sha256").update(data).digest("hex");
}

function hashPath(hash: string): string {
	return `${hash.slice(0, 2)}/${hash.slice(2, 4)}/${hash}`;
}
