/**
 * LocalFileStore — local filesystem storage for dev / tests.
 */
import { createHash } from "node:crypto";
import { dirname } from "node:path";
import { mkdir } from "node:fs/promises";
import type { FileRef, FileStore } from "@nexal/tape";
import type { StorageConfig } from "../config.ts";

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
		await mkdir(dirname(fullPath), { recursive: true });
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

function hashPath(hash: string): string {
	return `${hash.slice(0, 2)}/${hash.slice(2, 4)}/${hash}`;
}
