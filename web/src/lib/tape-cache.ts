export interface CachedTapeInfo {
	id: string;
	entries: number;
	anchors: number;
	lastAnchor: string | null;
	entriesSinceLastAnchor: number;
	lastTokenUsage: number | null;
}

export interface CachedTapeEntry {
	id: number;
	kind: string;
	payload: Record<string, unknown>;
	meta: Record<string, unknown>;
	date: string;
}

interface TapeEntryRecord {
	key: string;
	tapeId: string;
	entryId: number;
	entry: CachedTapeEntry;
	cachedAt: number;
}

const DB_NAME = "nexal-tape-cache";
const DB_VERSION = 1;
const TAPES_STORE = "tapes";
const ENTRIES_STORE = "entries";
const ENTRY_INDEX = "byTapeAndEntry";

let dbPromise: Promise<IDBDatabase> | null = null;

function openDb(): Promise<IDBDatabase> {
	if (!("indexedDB" in globalThis)) {
		return Promise.reject(new Error("IndexedDB unavailable"));
	}
	if (dbPromise) return dbPromise;
	dbPromise = new Promise((resolve, reject) => {
		const request = indexedDB.open(DB_NAME, DB_VERSION);
		request.onerror = () => reject(request.error ?? new Error("Failed to open tape cache"));
		request.onupgradeneeded = () => {
			const db = request.result;
			if (!db.objectStoreNames.contains(TAPES_STORE)) {
				db.createObjectStore(TAPES_STORE, { keyPath: "id" });
			}
			if (!db.objectStoreNames.contains(ENTRIES_STORE)) {
				const entries = db.createObjectStore(ENTRIES_STORE, { keyPath: "key" });
				entries.createIndex(ENTRY_INDEX, ["tapeId", "entryId"], { unique: true });
			}
		};
		request.onsuccess = () => resolve(request.result);
	});
	return dbPromise;
}

function requestToPromise<T>(request: IDBRequest<T>): Promise<T> {
	return new Promise((resolve, reject) => {
		request.onerror = () => reject(request.error ?? new Error("IndexedDB request failed"));
		request.onsuccess = () => resolve(request.result);
	});
}

function transactionDone(tx: IDBTransaction): Promise<void> {
	return new Promise((resolve, reject) => {
		tx.oncomplete = () => resolve();
		tx.onerror = () => reject(tx.error ?? new Error("IndexedDB transaction failed"));
		tx.onabort = () => reject(tx.error ?? new Error("IndexedDB transaction aborted"));
	});
}

function entryKey(tapeId: string, entryId: number): string {
	return `${tapeId}:${entryId.toString().padStart(12, "0")}`;
}

export async function getCachedTapeInfo(id: string): Promise<CachedTapeInfo | null> {
	try {
		const db = await openDb();
		const tx = db.transaction(TAPES_STORE, "readonly");
		return (await requestToPromise<CachedTapeInfo | undefined>(
			tx.objectStore(TAPES_STORE).get(id),
		)) ?? null;
	} catch {
		return null;
	}
}

export async function getCachedTapes(): Promise<CachedTapeInfo[]> {
	try {
		const db = await openDb();
		const tx = db.transaction(TAPES_STORE, "readonly");
		return await requestToPromise<CachedTapeInfo[]>(
			tx.objectStore(TAPES_STORE).getAll(),
		);
	} catch {
		return [];
	}
}

export async function putCachedTapeInfo(info: CachedTapeInfo): Promise<void> {
	try {
		const db = await openDb();
		const tx = db.transaction(TAPES_STORE, "readwrite");
		tx.objectStore(TAPES_STORE).put(info);
		await transactionDone(tx);
	} catch {
		// Cache writes are opportunistic.
	}
}

export async function putCachedTapes(tapes: CachedTapeInfo[]): Promise<void> {
	try {
		const db = await openDb();
		const tx = db.transaction(TAPES_STORE, "readwrite");
		const store = tx.objectStore(TAPES_STORE);
		store.clear();
		for (const tape of tapes) store.put(tape);
		await transactionDone(tx);
	} catch {
		// Cache writes are opportunistic.
	}
}

export async function deleteCachedTape(tapeId: string): Promise<void> {
	try {
		const db = await openDb();
		const tx = db.transaction([TAPES_STORE, ENTRIES_STORE], "readwrite");
		tx.objectStore(TAPES_STORE).delete(tapeId);
		const entries = tx.objectStore(ENTRIES_STORE);
		const index = entries.index(ENTRY_INDEX);
		const range = IDBKeyRange.bound([tapeId, 0], [tapeId, Number.MAX_SAFE_INTEGER]);
		const request = index.openCursor(range);
		request.onsuccess = () => {
			const cursor = request.result;
			if (!cursor) return;
			cursor.delete();
			cursor.continue();
		};
		await transactionDone(tx);
	} catch {
		// Cache deletes are opportunistic.
	}
}

export async function putCachedTapeEntries(
	tapeId: string,
	entries: CachedTapeEntry[],
): Promise<void> {
	if (entries.length === 0) return;
	try {
		const db = await openDb();
		const tx = db.transaction(ENTRIES_STORE, "readwrite");
		const store = tx.objectStore(ENTRIES_STORE);
		const cachedAt = Date.now();
		for (const entry of entries) {
			store.put({
				key: entryKey(tapeId, entry.id),
				tapeId,
				entryId: entry.id,
				entry,
				cachedAt,
			} satisfies TapeEntryRecord);
		}
		await transactionDone(tx);
	} catch {
		// Cache writes are opportunistic.
	}
}

export async function getCachedTapeEntriesPage(
	tapeId: string,
	offset: number,
	limit: number,
): Promise<CachedTapeEntry[]> {
	if (limit <= 0) return [];
	const lower = offset + 1;
	const upper = offset + limit;
	return readEntryRange(tapeId, IDBKeyRange.bound([tapeId, lower], [tapeId, upper]), "next", limit);
}

export async function getCachedLatestTapeEntries(
	tapeId: string,
	limit: number,
): Promise<CachedTapeEntry[]> {
	if (limit <= 0) return [];
	const entries = await readEntryRange(
		tapeId,
		IDBKeyRange.bound([tapeId, 0], [tapeId, Number.MAX_SAFE_INTEGER]),
		"prev",
		limit,
	);
	return entries.reverse();
}

async function readEntryRange(
	tapeId: string,
	range: IDBKeyRange,
	direction: IDBCursorDirection,
	limit: number,
): Promise<CachedTapeEntry[]> {
	try {
		const db = await openDb();
		const tx = db.transaction(ENTRIES_STORE, "readonly");
		const index = tx.objectStore(ENTRIES_STORE).index(ENTRY_INDEX);
		return await new Promise((resolve, reject) => {
			const out: CachedTapeEntry[] = [];
			const request = index.openCursor(range, direction);
			request.onerror = () => reject(request.error ?? new Error("Failed to read tape cache"));
			request.onsuccess = () => {
				const cursor = request.result;
				if (!cursor || out.length >= limit) {
					resolve(out);
					return;
				}
				const record = cursor.value as TapeEntryRecord;
				if (record.tapeId === tapeId) out.push(record.entry);
				cursor.continue();
			};
		});
	} catch {
		return [];
	}
}
