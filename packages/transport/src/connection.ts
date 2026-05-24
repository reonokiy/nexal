/**
 * Connection — high-level multiplexed message layer over a Transport.
 *
 * Supports virtual streams: multiple logical channels over one WS
 * connection. Each stream has its own request/response correlation
 * and notification handlers.
 *
 * @example
 * ```ts
 * const conn = new Connection(transport);
 *
 * // connection-level requests
 * const hello = await conn.request("gateway/hello", { ... });
 *
 * // virtual streams for agents
 * const agent = conn.stream("agent-1");
 * const result = await agent.request("process/start", { ... });
 * agent.on("process/output", (params) => { ... });
 * agent.close();
 * ```
 */
import { encodeFrame, decodeFrame } from "./codec.ts";
import { isWireRequest, isWireResponse, isWireNotification } from "./wire.ts";
import type { WireRequest, WireResponse, WireNotification, WireError, WireMessage, MessageId } from "./wire.ts";
import type { Transport } from "./transport.ts";

export type RequestHandler = (params: unknown) => unknown | Promise<unknown>;

interface Pending {
	resolve: (v: unknown) => void;
	reject: (err: Error) => void;
}

export class Connection {
	private readonly pending = new Map<MessageId, Pending>();
	private readonly handlers = new Map<string, Set<(params: unknown) => void>>();
	private readonly requestHandlers = new Map<string, RequestHandler>();
	private readonly streams = new Map<string, Stream>();

	constructor(private readonly transport: Transport) {}

	// ── Sending ───────────────────────────────────────────────────────

	/** Send a connection-level request and wait for a response. */
	async request(method: string, params?: unknown): Promise<unknown> {
		const id = crypto.randomUUID();
		const promise = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		this.send({ id, method, params });
		return promise;
	}

	/** Send a connection-level notification (fire-and-forget). */
	notify(method: string, params?: unknown): void {
		this.send({ method, params });
	}

	/** Send a raw wire message on this connection. */
	send(msg: WireRequest | WireResponse | WireNotification): void {
		this.transport.send(encodeFrame(msg));
	}

	// ── Streams ───────────────────────────────────────────────────────

	/** Open a virtual stream. Multiple streams share one transport. */
	stream(id: string): Stream {
		let s = this.streams.get(id);
		if (!s) {
			s = new Stream(this, id);
			this.streams.set(id, s);
		}
		return s;
	}

	/** Close a virtual stream and reject its pending requests. */
	closeStream(id: string): void {
		const s = this.streams.get(id);
		if (!s) return;
		s.rejectAllPending(new Error(`stream ${id} closed`));
		this.streams.delete(id);
	}

	// ── Receiving ─────────────────────────────────────────────────────

	/**
	 * Handle incoming frames. Wire this to your transport's onMessage.
	 *
	 * Routing:
	 *   - messages with `stream` → routed to that Stream
	 *   - messages without `stream` → handled at connection level
	 */
	handleMessage(data: Uint8Array): void {
		let msg: Record<string, unknown>;
		try {
			const decoded = decodeFrame<unknown>(data);
			if (!decoded || typeof decoded !== "object" || Array.isArray(decoded)) return;
			msg = decoded as Record<string, unknown>;
		} catch {
			return;
		}
		const streamId = msg.stream as string | undefined;

		if (streamId) {
			const s = this.streams.get(streamId);
			if (s) s.handleMessage(msg);
			return;
		}

		this.dispatch(msg);
	}

	/** @internal Dispatch a message at connection or stream level. */
	dispatch(msg: Record<string, unknown>): void {
		if (isWireResponse(msg as unknown as WireMessage)) {
			const resp = msg as unknown as WireResponse;
			const slot = this.pending.get(resp.id) ?? this.pending.get(String(resp.id));
			if (!slot) return;
			this.pending.delete(resp.id);
			this.pending.delete(String(resp.id));
			if (resp.error) {
				slot.reject(new WireErrorMessage(resp.error));
			} else {
				slot.resolve(resp.result);
			}
			return;
		}

		if (isWireNotification(msg as unknown as WireMessage)) {
			const notif = msg as unknown as WireNotification;
			const set = this.handlers.get(notif.method);
			if (!set) return;
			for (const handler of set) {
				try {
					handler(notif.params);
				} catch (err) {
					console.error(`notification handler threw for ${notif.method}`, err);
				}
			}
			return;
		}

		if (isWireRequest(msg as unknown as WireMessage)) {
			const req = msg as unknown as WireRequest;
			const handler = this.requestHandlers.get(req.method);
			if (!handler) return;
			Promise.resolve(handler(req.params)).then(
				(result) => {
					this.send({ id: req.id, result } as WireResponse);
				},
				(err) => {
					const error: WireError = {
						code: -32000,
						message: err instanceof Error ? err.message : String(err),
					};
					this.send({ id: req.id, error } as WireResponse);
				},
			);
		}
	}

	/** Subscribe to a connection-level notification. */
	on(method: string, handler: (params: unknown) => void): () => void {
		let set = this.handlers.get(method);
		if (!set) {
			set = new Set();
			this.handlers.set(method, set);
		}
		set.add(handler);
		return () => set!.delete(handler);
	}

	/** Register a connection-level request handler (bidirectional RPC). */
	handleRequest(method: string, handler: RequestHandler): void {
		this.requestHandlers.set(method, handler);
	}

	// ── Lifecycle ─────────────────────────────────────────────────────

	close(): void {
		this.rejectAllPending(new Error("connection closed"));
		for (const s of this.streams.values()) s.rejectAllPending(new Error("connection closed"));
		this.streams.clear();
		this.transport.close();
	}

	private rejectAllPending(reason: Error): void {
		for (const p of this.pending.values()) p.reject(reason);
		this.pending.clear();
	}
}

/**
 * Stream — a virtual channel multiplexed over a Connection.
 *
 * Each stream has independent request/response correlation and
 * notification handlers. Messages are tagged with `stream` so
 * the remote side can route them.
 */
export class Stream {
	private readonly pending = new Map<MessageId, Pending>();
	private readonly handlers = new Map<string, Set<(params: unknown) => void>>();
	private readonly requestHandlers = new Map<string, RequestHandler>();

	constructor(
		private readonly conn: Connection,
		readonly id: string,
	) {}

	// ── Sending ───────────────────────────────────────────────────────

	/** Send a request on this stream and wait for a response. */
	async request(method: string, params?: unknown): Promise<unknown> {
		const id = crypto.randomUUID();
		const promise = new Promise<unknown>((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
		});
		this.send({ stream: this.id, id, method, params });
		return promise;
	}

	/** Send a notification on this stream (fire-and-forget). */
	notify(method: string, params?: unknown): void {
		this.send({ stream: this.id, method, params });
	}

	/** Send a raw wire message on this stream. */
	send(msg: WireRequest | WireResponse | WireNotification): void {
		this.conn.send(msg);
	}

	// ── Receiving ─────────────────────────────────────────────────────

	/** @internal Route a message from the connection to this stream. */
	handleMessage(msg: Record<string, unknown>): void {
		if (isWireResponse(msg as unknown as WireMessage)) {
			const resp = msg as unknown as WireResponse;
			const slot = this.pending.get(resp.id) ?? this.pending.get(String(resp.id));
			if (!slot) return;
			this.pending.delete(resp.id);
			this.pending.delete(String(resp.id));
			if (resp.error) {
				slot.reject(new WireErrorMessage(resp.error));
			} else {
				slot.resolve(resp.result);
			}
			return;
		}

		if (isWireNotification(msg as unknown as WireMessage)) {
			const notif = msg as unknown as WireNotification;
			const set = this.handlers.get(notif.method);
			if (!set) return;
			for (const handler of set) {
				try {
					handler(notif.params);
				} catch (err) {
					console.error(`stream ${this.id} notification handler threw for ${notif.method}`, err);
				}
			}
			return;
		}

		if (isWireRequest(msg as unknown as WireMessage)) {
			const req = msg as unknown as WireRequest;
			const handler = this.requestHandlers.get(req.method);
			if (!handler) return;
			Promise.resolve(handler(req.params)).then(
				(result) => {
					this.send({ stream: this.id, id: req.id, result } as WireResponse);
				},
				(err) => {
					const error: WireError = {
						code: -32000,
						message: err instanceof Error ? err.message : String(err),
					};
					this.send({ stream: this.id, id: req.id, error } as WireResponse);
				},
			);
		}
	}

	/** Subscribe to a notification on this stream. */
	on(method: string, handler: (params: unknown) => void): () => void {
		let set = this.handlers.get(method);
		if (!set) {
			set = new Set();
			this.handlers.set(method, set);
		}
		set.add(handler);
		return () => set!.delete(handler);
	}

	/** Register a request handler on this stream (bidirectional RPC). */
	handleRequest(method: string, handler: RequestHandler): void {
		this.requestHandlers.set(method, handler);
	}

	// ── Lifecycle ─────────────────────────────────────────────────────

	/** Close this stream. Other streams and the connection stay open. */
	close(): void {
		this.rejectAllPending(new Error(`stream ${this.id} closed`));
		this.conn.closeStream(this.id);
	}

	/** @internal Reject all pending requests for this stream. */
	rejectAllPending(reason: Error): void {
		for (const p of this.pending.values()) p.reject(reason);
		this.pending.clear();
	}
}

export class WireErrorMessage extends Error {
	constructor(readonly error: WireError) {
		super(error.message);
		this.name = "WireErrorMessage";
	}
}
