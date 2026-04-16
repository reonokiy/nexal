import { afterEach, beforeEach, describe, expect, test } from "bun:test";

import { GatewayClient, GatewayError } from "./client.ts";

/**
 * The GatewayClient uses the global `WebSocket` constructor and reads
 * `WebSocket.OPEN` for state checks. We stub the global with a hand-rolled
 * fake that lets tests drive open/message/close events synchronously and
 * observe outgoing frames.
 */
class FakeWebSocket {
	static CONNECTING = 0;
	static OPEN = 1;
	static CLOSING = 2;
	static CLOSED = 3;

	/** Last constructed instance — tests read this to grab the WS. */
	static last: FakeWebSocket | null = null;
	/** All instances created during a test (for multi-connect scenarios). */
	static instances: FakeWebSocket[] = [];

	readonly url: string;
	readyState = FakeWebSocket.CONNECTING;
	readonly listeners: Record<string, Array<(ev: any) => void>> = {};
	readonly sent: string[] = [];

	constructor(url: string) {
		this.url = url;
		FakeWebSocket.last = this;
		FakeWebSocket.instances.push(this);
	}

	addEventListener(ev: string, fn: (e: any) => void): void {
		(this.listeners[ev] ??= []).push(fn);
	}

	send(data: string): void {
		this.sent.push(data);
	}

	close(): void {
		if (this.readyState === FakeWebSocket.CLOSED) return;
		this.readyState = FakeWebSocket.CLOSED;
		this.emit("close", { code: 1000 });
	}

	// ── Test-side drivers ─────────────────────────────────────────────
	openNow(): void {
		this.readyState = FakeWebSocket.OPEN;
		this.emit("open", {});
	}

	deliver(text: string): void {
		this.emit("message", { data: text });
	}

	errorOut(message = "boom"): void {
		this.emit("error", { message });
	}

	private emit(ev: string, data: any): void {
		for (const fn of this.listeners[ev] ?? []) fn(data);
	}

	/** Parse the most recent outgoing frame as JSON-RPC. */
	lastSent(): {
		jsonrpc: string;
		id?: string | number;
		method?: string;
		params?: any;
	} {
		const raw = this.sent[this.sent.length - 1];
		if (!raw) throw new Error("no frames sent");
		return JSON.parse(raw);
	}
}

const realWebSocket = globalThis.WebSocket;

beforeEach(() => {
	FakeWebSocket.last = null;
	FakeWebSocket.instances = [];
	(globalThis as any).WebSocket = FakeWebSocket;
});

afterEach(() => {
	(globalThis as any).WebSocket = realWebSocket;
});

function newClient(opts: Partial<ConstructorParameters<typeof GatewayClient>[0]> = {}) {
	return new GatewayClient({
		url: "ws://127.0.0.1:5500",
		token: "tok",
		clientName: "test-client",
		connectTimeoutMs: 500,
		...opts,
	});
}

/** Respond to the most recent JSON-RPC request with a successful result. */
function respondOk(ws: FakeWebSocket, result: unknown): void {
	const sent = ws.lastSent();
	ws.deliver(
		JSON.stringify({ jsonrpc: "2.0", id: sent.id, result }),
	);
}

/**
 * Flush pending microtasks. `async` wrappers + adopted-Promise state give
 * each `await` a microtask hop, so a single `await Promise.resolve()` isn't
 * enough to drain hello()'s nested `await this.connect() → await this.invoke(...)`.
 */
async function flush(): Promise<void> {
	for (let i = 0; i < 10; i++) await Promise.resolve();
}

describe("GatewayClient", () => {
	describe("connect()", () => {
		test("resolves when the WS opens", async () => {
			const client = newClient();
			const promise = client.connect();
			// Instance should exist synchronously.
			expect(FakeWebSocket.last).not.toBeNull();
			expect(FakeWebSocket.last!.url).toBe("ws://127.0.0.1:5500");
			FakeWebSocket.last!.openNow();
			await promise;
		});

		test("is idempotent — second call reuses the first connection", async () => {
			const client = newClient();
			const p1 = client.connect();
			const p2 = client.connect();
			// Only one underlying WebSocket got created — this is what
			// "idempotent" means on the wire. (The `async` wrapper around
			// `connect()` produces a new outer Promise per call, so we
			// can't assert `p1 === p2`.)
			expect(FakeWebSocket.instances).toHaveLength(1);
			FakeWebSocket.last!.openNow();
			await p1;
			await p2;
		});

		test("rejects on error event", async () => {
			const client = newClient();
			const promise = client.connect();
			FakeWebSocket.last!.errorOut("nope");
			await expect(promise).rejects.toThrow(/gateway WS error/);
		});

		test("rejects if handshake doesn't happen before timeout", async () => {
			const client = newClient({ connectTimeoutMs: 20 });
			await expect(client.connect()).rejects.toThrow(/timed out/);
		});
	});

	describe("invoke()", () => {
		test("sends a JSON-RPC request and resolves with the result", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const pending = client.invoke("gateway/list_agents", {});
			const ws = FakeWebSocket.last!;
			const sent = ws.lastSent();
			expect(sent.jsonrpc).toBe("2.0");
			expect(sent.method).toBe("gateway/list_agents");
			expect(typeof sent.id).toBe("string");

			respondOk(ws, { agents: [] });
			await expect(pending).resolves.toEqual({ agents: [] });
		});

		test("rejects with GatewayError when the server returns an error", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const pending = client.invoke("gateway/kill_agent", { agent_id: "x" });
			const ws = FakeWebSocket.last!;
			const id = ws.lastSent().id;
			ws.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					id,
					error: { code: -32001, message: "no such agent", data: { agent: "x" } },
				}),
			);
			const err = await pending.catch((e) => e);
			expect(err).toBeInstanceOf(GatewayError);
			expect((err as GatewayError).code).toBe(-32001);
			expect((err as GatewayError).data).toEqual({ agent: "x" });
		});

		test("throws synchronously when called before connect()", () => {
			const client = newClient();
			expect(() =>
				client.invoke("gateway/list_agents", {}),
			).toThrow(/WS not connected/);
		});

		test("out-of-band responses (unknown id) are silently dropped", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			// Send a response for an id that was never requested. Must not throw.
			FakeWebSocket.last!.deliver(
				JSON.stringify({ jsonrpc: "2.0", id: "ghost", result: {} }),
			);
			// Now fire a real invoke and make sure it still works cleanly.
			const p = client.invoke("gateway/list_agents", {});
			respondOk(FakeWebSocket.last!, { agents: [] });
			await expect(p).resolves.toEqual({ agents: [] });
		});
	});

	describe("hello()", () => {
		test("sends gateway/hello with token + client name once, memoized", async () => {
			const client = newClient();
			const p1 = client.hello();
			// hello() calls connect() internally; drive the WS open.
			const ws = FakeWebSocket.last!;
			ws.openNow();
			// hello's IIFE awaits connect() (adopted Promise, 1 hop) then
			// calls invoke() which sends synchronously — flush microtasks
			// to let it progress.
			await flush();
			const sent = ws.lastSent();
			expect(sent.method).toBe("gateway/hello");
			expect(sent.params).toEqual({ token: "tok", client_name: "test-client" });
			respondOk(ws, { ok: true, gateway_version: "0.1.0" });
			await p1;

			// Second hello() returns the cached promise — no second frame.
			const framesBefore = ws.sent.length;
			const p2 = client.hello();
			expect(ws.sent.length).toBe(framesBefore);
			await p2;
		});
	});

	describe("invokeAgent()", () => {
		test("wraps the call in an agent/invoke envelope with agent_id + inner method", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const pending = client.invokeAgent("agent-42", "process/terminate", {
				process_id: "p1",
			});
			const ws = FakeWebSocket.last!;
			const sent = ws.lastSent();
			expect(sent.method).toBe("agent/invoke");
			expect(sent.params).toEqual({
				agent_id: "agent-42",
				method: "process/terminate",
				params: { process_id: "p1" },
			});
			respondOk(ws, { running: false });
			await expect(pending).resolves.toEqual({ running: false });
		});
	});

	describe("subscribe()", () => {
		test("dispatches agent/notify for known notification methods", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const seen: any[] = [];
			client.subscribe((n) => seen.push(n));
			FakeWebSocket.last!.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					method: "agent/notify",
					params: {
						agent_id: "a1",
						method: "process/output",
						params: { process_id: "p", stream: "stdout", chunk: "aGk=", seq: 1 },
					},
				}),
			);
			expect(seen).toHaveLength(1);
			expect(seen[0].agentId).toBe("a1");
			expect(seen[0].method).toBe("process/output");
			expect(seen[0].params.chunk).toBe("aGk=");
		});

		test("unknown methods fall through as UnknownAgentNotification (same shape)", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const seen: any[] = [];
			client.subscribe((n) => seen.push(n));
			FakeWebSocket.last!.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					method: "agent/notify",
					params: {
						agent_id: "a1",
						method: "future/event",
						params: { whatever: true },
					},
				}),
			);
			expect(seen).toHaveLength(1);
			expect(seen[0].method).toBe("future/event");
		});

		test("non-agent/notify notifications are ignored", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const seen: any[] = [];
			client.subscribe((n) => seen.push(n));
			FakeWebSocket.last!.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					method: "gateway/welcome",
					params: { hi: true },
				}),
			);
			expect(seen).toEqual([]);
		});

		test("notifications missing agent_id/method are ignored", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const seen: any[] = [];
			client.subscribe((n) => seen.push(n));
			FakeWebSocket.last!.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					method: "agent/notify",
					params: { agent_id: "only-id" }, // no `method`
				}),
			);
			FakeWebSocket.last!.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					method: "agent/notify",
					params: { method: "only-method" }, // no `agent_id`
				}),
			);
			expect(seen).toEqual([]);
		});

		test("a throwing handler does not break its siblings", async () => {
			const origError = console.error;
			(console as any).error = () => undefined;
			try {
				const client = newClient();
				const ready = client.connect();
				FakeWebSocket.last!.openNow();
				await ready;

				const seen: string[] = [];
				client.subscribe(() => {
					throw new Error("boom in handler 1");
				});
				client.subscribe((n) => seen.push(n.method));
				FakeWebSocket.last!.deliver(
					JSON.stringify({
						jsonrpc: "2.0",
						method: "agent/notify",
						params: {
							agent_id: "a1",
							method: "process/closed",
							params: { process_id: "p" },
						},
					}),
				);
				expect(seen).toEqual(["process/closed"]);
			} finally {
				(console as any).error = origError;
			}
		});

		test("unsubscribe removes the handler", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const seen: any[] = [];
			const off = client.subscribe((n) => seen.push(n));
			off();
			FakeWebSocket.last!.deliver(
				JSON.stringify({
					jsonrpc: "2.0",
					method: "agent/notify",
					params: {
						agent_id: "a1",
						method: "process/exited",
						params: { process_id: "p", exit_code: 0 },
					},
				}),
			);
			expect(seen).toEqual([]);
		});
	});

	describe("dispatch error paths", () => {
		test("non-JSON frames are dropped without throwing", async () => {
			const origError = console.error;
			(console as any).error = () => undefined;
			try {
				const client = newClient();
				const ready = client.connect();
				FakeWebSocket.last!.openNow();
				await ready;

				// Send garbage; the client must survive and keep serving the next invoke.
				FakeWebSocket.last!.deliver("not-json{");
				const p = client.invoke("gateway/list_agents", {});
				respondOk(FakeWebSocket.last!, { agents: [] });
				await expect(p).resolves.toEqual({ agents: [] });
			} finally {
				(console as any).error = origError;
			}
		});
	});

	describe("close() / lifecycle", () => {
		test("close() rejects pending invokes and resets state", async () => {
			const client = newClient();
			const ready = client.connect();
			const ws = FakeWebSocket.last!;
			ws.openNow();
			await ready;

			const pending = client.invoke("gateway/list_agents", {});
			// Close without replying — pending should reject.
			ws.close();
			await expect(pending).rejects.toThrow(/WS closed/);

			// After close, a new connect() spins up a fresh WS.
			const ready2 = client.connect();
			expect(FakeWebSocket.instances.length).toBe(2);
			FakeWebSocket.instances[1]!.openNow();
			await ready2;
		});

		test("explicit close() clears hello memoization so next hello re-runs", async () => {
			const client = newClient();
			const ready = client.connect();
			FakeWebSocket.last!.openNow();
			await ready;

			const h1 = client.hello();
			const ws1 = FakeWebSocket.last!;
			await flush();
			respondOk(ws1, { ok: true, gateway_version: "0.1.0" });
			await h1;

			// close() should drop readyPromise + helloPromise so a fresh
			// hello() triggers a new WS + fresh gateway/hello.
			await client.close();
			const h2 = client.hello();
			expect(FakeWebSocket.instances.length).toBe(2);
			FakeWebSocket.instances[1]!.openNow();
			await flush();
			respondOk(FakeWebSocket.instances[1]!, { ok: true, gateway_version: "0.1.0" });
			await h2;
		});
	});
});
