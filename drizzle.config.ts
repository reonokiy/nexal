/// <reference types="bun" />
import { defineConfig } from "drizzle-kit";

export default defineConfig({
	dialect: "postgresql",
	schema: "./src/schema.ts",
	out: "./drizzle",
	dbCredentials: {
		url: process.env.NEXAL_WORKERS_URL!,
	},
});
