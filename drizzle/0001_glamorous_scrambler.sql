CREATE TABLE "tape_entries" (
	"tape_id" integer NOT NULL,
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
CREATE TABLE "tape_files" (
	"id" serial PRIMARY KEY NOT NULL,
	"file_hash" varchar(64) NOT NULL,
	"storage_type" text NOT NULL,
	"bucket" text NOT NULL,
	"storage_path" text NOT NULL,
	"size_bytes" integer NOT NULL,
	"mime_type" text NOT NULL,
	"created_at" bigint NOT NULL,
	CONSTRAINT "tape_files_file_hash_unique" UNIQUE("file_hash")
);
--> statement-breakpoint
CREATE TABLE "tapes" (
	"id" serial PRIMARY KEY NOT NULL,
	"name" text NOT NULL,
	"name_key" varchar(64) NOT NULL,
	"last_entry_id" integer DEFAULT 0 NOT NULL,
	"created_at" bigint NOT NULL,
	CONSTRAINT "tapes_name_key_unique" UNIQUE("name_key")
);
--> statement-breakpoint
ALTER TABLE "tape_entries" ADD CONSTRAINT "tape_entries_tape_id_tapes_id_fk" FOREIGN KEY ("tape_id") REFERENCES "public"."tapes"("id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "idx_tape_entries_kind" ON "tape_entries" USING btree ("tape_id","kind","entry_id");--> statement-breakpoint
CREATE INDEX "idx_tape_entries_anchor" ON "tape_entries" USING btree ("tape_id","anchor_name_key","entry_id");--> statement-breakpoint
CREATE INDEX "idx_tape_files_hash" ON "tape_files" USING btree ("file_hash");