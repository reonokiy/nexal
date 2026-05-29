export const TapeEvent = {
	System: {
		Prompt: "system/prompt",
		Model: "system/model",
		Tools: "system/tools",
		Context: "system/context",
		Policy: "system/policy",
		Runtime: "system/runtime",
		Status: "system/status",
		Summary: "system/summary",
	},
} as const;

export const TapeStatus = {
	Missing: "missing",
	Current: "current",
	Changed: "changed",
} as const;

export const TapeChange = {
	Set: "set",
	Changed: "changed",
} as const;

export type TapeSystemEventType = typeof TapeEvent.System[keyof typeof TapeEvent.System];
export type TapeConfigStatus = typeof TapeStatus[keyof typeof TapeStatus];
export type TapeChange = typeof TapeChange[keyof typeof TapeChange];
