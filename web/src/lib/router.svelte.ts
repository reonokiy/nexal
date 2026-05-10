/**
 * Tiny hash router. Routes are slugs after `#/` — `#/`, `#/settings`.
 * Reactive `route` rune kept in sync with `window.location.hash`.
 */

function parse(): string {
	const h = (typeof window !== "undefined" ? window.location.hash : "") || "#/";
	return h.replace(/^#\//, "").replace(/\/+$/, "") || "home";
}

let routeValue = $state(parse());

if (typeof window !== "undefined") {
	window.addEventListener("hashchange", () => {
		routeValue = parse();
	});
}

export function getRoute(): string {
	return routeValue;
}

export const router = {
	get current() {
		return routeValue;
	},
	go(to: string) {
		const slug = to.replace(/^#?\/?/, "");
		const target = slug === "home" || slug === "" ? "#/" : `#/${slug}`;
		if (typeof window !== "undefined" && window.location.hash !== target) {
			window.location.hash = target;
		}
		routeValue = parse();
	},
};
