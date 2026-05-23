/**
 * applyEdits — apply redactions and amendments to tape entries.
 *
 * - Redacted entries → excluded
 * - Amended entries → replaced with new entries
 * - All history preserved in tape
 */
import type { TapeEntry } from "./types.ts";

export function applyEdits(entries: TapeEntry[]): TapeEntry[] {
	const redacted = new Set<number>();
	const amendments: Array<{ targetIds: Set<number>; replacement: TapeEntry[] }> = [];

	for (const entry of entries) {
		if (entry.kind === "redaction") {
			const targetId = entry.payload.targetId as number;
			if (targetId) redacted.add(targetId);
		}
		if (entry.kind === "amendment") {
			const targetIds = entry.payload.targetIds as number[];
			const replacement = entry.payload.replacement as TapeEntry[];
			if (targetIds?.length && replacement?.length) {
				amendments.push({
					targetIds: new Set(targetIds),
					replacement,
				});
			}
		}
	}

	if (redacted.size === 0 && amendments.length === 0) return entries;

	const amendedIds = new Set<number>();
	for (const a of amendments) {
		for (const id of a.targetIds) {
			amendedIds.add(id);
		}
	}

	const result: TapeEntry[] = [];
	let i = 0;

	while (i < entries.length) {
		const entry = entries[i]!;

		if (entry.kind === "redaction" || entry.kind === "amendment") {
			i++;
			continue;
		}

		if (redacted.has(entry.id)) {
			i++;
			continue;
		}

		if (amendedIds.has(entry.id)) {
			const amendment = amendments.find((a) => a.targetIds.has(entry.id));
			if (amendment) {
				result.push(...amendment.replacement);
				for (const id of amendment.targetIds) {
					amendedIds.delete(id);
				}
				const maxId = Math.max(...amendment.targetIds);
				while (i < entries.length && entries[i]!.id <= maxId) {
					i++;
				}
				continue;
			}
		}

		result.push(entry);
		i++;
	}

	return result;
}
