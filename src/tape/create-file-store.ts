/**
 * FileStore factory — picks the right implementation from config.
 */
import type { FileStore } from "@nexal/tape";
import type { StorageConfig } from "../config.ts";
import { S3FileStore } from "./s3-file-store.ts";
import { LocalFileStore } from "./local-file-store.ts";

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
