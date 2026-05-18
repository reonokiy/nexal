/**
 * Supabase Auth JWT verification.
 *
 * Uses jose to verify JWTs against Supabase's JWKS endpoint.
 * Tokens are issued by supabase-js on the client side (web UI)
 * and verified here before allowing WS/HTTP access.
 */
import * as jose from "jose";
import { createLog } from "./log.ts";

const log = createLog("auth");

export interface AuthUser {
	sub: string;
	email?: string;
	role?: string;
}

const SUPABASE_URL =
	process.env.NEXAL_SUPABASE_URL ??
	"https://oiucjptwjncfbzotwgbg.supabase.co";
const JWKS_URL = `${SUPABASE_URL}/auth/v1/.well-known/jwks.json`;

let _jwks: ReturnType<typeof jose.createRemoteJWKSet> | null = null;

function getJWKS() {
	if (!_jwks) {
		_jwks = jose.createRemoteJWKSet(new URL(JWKS_URL));
	}
	return _jwks;
}

/** Verify a Supabase access token. Returns user info or null. */
export async function verifySupabaseJwt(
	token: string,
): Promise<AuthUser | null> {
	try {
		const { payload } = await jose.jwtVerify(token, getJWKS(), {
			audience: "authenticated",
		});
		if (!payload.sub) return null;
		return {
			sub: payload.sub,
			email: payload.email as string | undefined,
			role: payload.role as string | undefined,
		};
	} catch (err) {
		log.error("jwt verification failed", err);
		return null;
	}
}

/** Whether auth enforcement is enabled. */
export function isAuthEnabled(): boolean {
	return process.env.NEXAL_AUTH_ENABLED === "true";
}
