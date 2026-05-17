CREATE TABLE "settings" (
	"key" text PRIMARY KEY NOT NULL,
	"value" text NOT NULL
);
--> statement-breakpoint
CREATE TABLE "workers" (
	"id" text PRIMARY KEY NOT NULL,
	"kind" text NOT NULL,
	"lifetime" text NOT NULL,
	"parent_session_key" text NOT NULL,
	"source_channel" text NOT NULL,
	"source_chat_id" text NOT NULL,
	"source_reply_to" text,
	"name" text NOT NULL,
	"initial_prompt" text,
	"system_prompt" text NOT NULL,
	"model_provider" text NOT NULL,
	"model_id" text NOT NULL,
	"status" text NOT NULL,
	"messages_json" text DEFAULT '[]' NOT NULL,
	"container_name" text NOT NULL,
	"created_at" bigint NOT NULL,
	"started_at" bigint,
	"updated_at" bigint NOT NULL,
	"completed_at" bigint,
	"error" text,
	"turn_count" integer DEFAULT 0 NOT NULL,
	"send_policy" text DEFAULT 'explicit' NOT NULL
);
--> statement-breakpoint
CREATE INDEX "workers_status_idx" ON "workers" USING btree ("status");--> statement-breakpoint
CREATE INDEX "workers_parent_idx" ON "workers" USING btree ("parent_session_key");--> statement-breakpoint
UPDATE "workers" SET "lifetime" = 'oneshot' WHERE "lifetime" = 'shot';