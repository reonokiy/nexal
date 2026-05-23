DROP TABLE "tape_entries";--> statement-breakpoint
DROP TABLE "tapes";--> statement-breakpoint
CREATE TABLE "tapes" (
	"id" uuid PRIMARY KEY NOT NULL,
	"last_entry_id" integer DEFAULT 0 NOT NULL,
	"created_at" bigint NOT NULL
);
--> statement-breakpoint
CREATE TABLE "tape_entries" (
	"tape_id" uuid NOT NULL,
	"entry_id" integer NOT NULL,
	"kind" text NOT NULL,
	"anchor_name" text,
	"anchor_name_key" varchar(64),
	"payload" jsonb NOT NULL,
	"meta" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"entry_date" text NOT NULL,
	"created_at" bigint NOT NULL,
	CONSTRAINT "tape_entries_tape_id_entry_id_pk" PRIMARY KEY("tape_id","entry_id")
);
--> statement-breakpoint
CREATE TABLE "session_tapes" (
	"session_key" text PRIMARY KEY NOT NULL,
	"tape_id" uuid NOT NULL,
	"created_at" bigint NOT NULL,
	"updated_at" bigint NOT NULL
);
--> statement-breakpoint
ALTER TABLE "workers" ADD COLUMN "tape_id" uuid;--> statement-breakpoint
ALTER TABLE "tape_entries" ADD CONSTRAINT "tape_entries_tape_id_tapes_id_fk" FOREIGN KEY ("tape_id") REFERENCES "public"."tapes"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "session_tapes" ADD CONSTRAINT "session_tapes_tape_id_tapes_id_fk" FOREIGN KEY ("tape_id") REFERENCES "public"."tapes"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "workers" ADD CONSTRAINT "workers_tape_id_tapes_id_fk" FOREIGN KEY ("tape_id") REFERENCES "public"."tapes"("id") ON DELETE set null ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "idx_tape_entries_kind" ON "tape_entries" USING btree ("tape_id","kind","entry_id");--> statement-breakpoint
CREATE INDEX "idx_tape_entries_anchor" ON "tape_entries" USING btree ("tape_id","anchor_name_key","entry_id");
