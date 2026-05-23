import type { TelegramUser } from "./types.ts";

const TG = "https://api.telegram.org";

export async function apiCall<T>(
	token: string,
	method: string,
	params: Record<string, unknown>,
): Promise<T> {
	const url = `${TG}/bot${token}/${method}`;
	const resp = await fetch(url, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify(params),
	});
	const body = (await resp.json()) as { ok: boolean; result?: T; description?: string };
	if (!body.ok) throw new Error(`telegram ${method}: ${body.description ?? resp.status}`);
	return body.result as T;
}

export async function downloadFile(token: string, fileId: string): Promise<Buffer> {
	const file = await apiCall<{ file_path: string }>(token, "getFile", { file_id: fileId });
	const url = `${TG}/file/bot${token}/${file.file_path}`;
	const resp = await fetch(url);
	if (!resp.ok) throw new Error(`getFile download failed: ${resp.status}`);
	return Buffer.from(await resp.arrayBuffer());
}

export async function getMe(token: string): Promise<TelegramUser> {
	return apiCall<TelegramUser>(token, "getMe", {});
}
