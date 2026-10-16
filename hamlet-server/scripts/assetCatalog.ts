/**
 * The pure parts of `pnpm assets:publish`: reading the baker's catalog,
 * naming what goes to R2 and writing the SQL that upserts D1. Nothing here
 * runs Wrangler, so the tests can use all of it.
 */
import { existsSync } from "node:fs";
import { extname, join } from "node:path";
import { z } from "zod";
import {
  ASSET_CATEGORIES,
  ASSET_ID_PATTERN,
  ASSET_TARGETS,
  MAX_FRAMES,
  assetFileKey,
  assetPreviewKey,
} from "../src/assetKeys.ts";
import { describeZodError } from "../src/errors.ts";
import { MAX_PRICE_CENTS, MIN_PAID_CENTS, isValidPrice } from "../src/money.ts";
import { parseVersion } from "../src/versionNumber.ts";

/** The content type a known file extension must be published with. */
const EXTENSION_TYPES: Record<string, string> = {
  ".glb": "model/gltf-binary",
  ".json": "application/json",
};
const CONTENT_TYPE_PATTERN = /^[a-z]+\/[a-z0-9.+-]+$/;
// One line each, so every SQL statement stays on one line.
const NO_CONTROL_CHARS = /^[^\p{Cc}]*$/u;
const FILE_NAME_PATTERN = /^[A-Za-z0-9._-]+$/;

const entrySchema = z
  .object({
    id: z.string().regex(ASSET_ID_PATTERN, "lowercase letters, digits and -, at most 64"),
    target: z.enum(ASSET_TARGETS),
    category: z.enum(ASSET_CATEGORIES),
    title: z.string().trim().min(1).max(60).regex(NO_CONTROL_CHARS, "one line"),
    summary: z.string().trim().min(1).max(140).regex(NO_CONTROL_CHARS, "one line"),
    priceCents: z
      .number()
      .int()
      .refine(isValidPrice, `must be 0 or ${MIN_PAID_CENTS} to ${MAX_PRICE_CENTS}`),
    version: z.string().refine((text) => parseVersion(text) !== null, "must be MAJOR.MINOR.PATCH"),
    // `files/<name>` or just `<name>`, inside the catalog's files/ folder.
    file: z
      .string()
      .transform((file) => file.replace(/^files\//, ""))
      .pipe(z.string().regex(FILE_NAME_PATTERN, "a plain file name in files/"))
      .refine((name) => name !== "." && name !== "..", "a plain file name in files/"),
    contentType: z.string().regex(CONTENT_TYPE_PATTERN, "a MIME type, e.g. model/gltf-binary"),
    frames: z.number().int().min(0).max(MAX_FRAMES),
  })
  .superRefine((entry, context) => {
    const expected = EXTENSION_TYPES[extname(entry.file).toLowerCase()];
    if (expected && entry.contentType !== expected) {
      context.addIssue({
        code: "custom",
        path: ["contentType"],
        message: `must be ${expected} for ${extname(entry.file)}`,
      });
    }
  });

export type CatalogEntry = z.infer<typeof entrySchema>;

/** Checks catalog.json's contents; throws with a readable message. */
export function parseCatalog(json: unknown): CatalogEntry[] {
  if (!Array.isArray(json)) throw new Error("catalog.json must be a JSON array");
  const entries = json.map((raw, index) => {
    const parsed = entrySchema.safeParse(raw);
    if (!parsed.success) throw new Error(describeZodError(parsed.error, `[${index}]`));
    return parsed.data;
  });
  const seen = new Set<string>();
  for (const entry of entries) {
    if (seen.has(entry.id)) throw new Error(`asset ${entry.id} is in the catalog twice`);
    seen.add(entry.id);
  }
  return entries;
}

/** One file to put in R2. */
export interface Upload {
  key: string;
  path: string;
  contentType: string;
}

/** Every file and preview of the catalog, as paths under `dir`. */
export function uploadsFor(dir: string, entries: CatalogEntry[]): Upload[] {
  return entries.flatMap((entry) => [
    {
      key: assetFileKey(entry.id, entry.version),
      path: join(dir, "files", entry.file),
      contentType: entry.contentType,
    },
    ...Array.from({ length: entry.frames }, (_, frame) => ({
      key: assetPreviewKey(entry.id, entry.version, frame),
      path: join(dir, "previews", entry.id, `${frame}.png`),
      contentType: "image/png",
    })),
  ]);
}

/** The uploads whose file is not there. */
export function missingFiles(
  uploads: Upload[],
  exists: (path: string) => boolean = existsSync,
): string[] {
  return uploads.filter((upload) => !exists(upload.path)).map((upload) => upload.path);
}

const ASSET_COLUMNS = [
  "id",
  "target",
  "category",
  "title",
  "summary",
  "priceCents",
  "version",
  "fileKey",
  "contentType",
  "frames",
] as const;

/**
 * Upserts every entry as listed and unlists every other asset. A row that
 * is already as published is left alone, `updatedAt` included, so running
 * it twice changes nothing.
 */
export function publishSql(entries: CatalogEntry[], now: Date): string {
  const time = sqlString(sqliteTime(now));
  const columns = [...ASSET_COLUMNS, "status"];
  const quoted = (names: readonly string[]) => names.map((name) => `"${name}"`).join(", ");
  const upserts = entries.map((entry) => {
    const values = [
      ...ASSET_COLUMNS.map((column) =>
        column === "fileKey"
          ? sqlString(assetFileKey(entry.id, entry.version))
          : sqlValue(entry[column]),
      ),
      "'listed'",
    ];
    const set = columns.map((column) => `"${column}" = excluded."${column}"`).join(", ");
    const changed = columns
      .map((column) => `"Asset"."${column}" IS NOT excluded."${column}"`)
      .join(" OR ");
    return (
      `INSERT INTO "Asset" (${quoted(columns)}, "createdAt", "updatedAt") ` +
      `VALUES (${values.join(", ")}, ${time}, ${time}) ` +
      `ON CONFLICT ("id") DO UPDATE SET ${set}, "updatedAt" = excluded."updatedAt" ` +
      `WHERE ${changed};`
    );
  });
  const keep =
    entries.length === 0
      ? ""
      : ` AND "id" NOT IN (${entries.map((e) => sqlString(e.id)).join(", ")})`;
  const unlist = `UPDATE "Asset" SET "status" = 'unlisted', "updatedAt" = ${time} WHERE "status" <> 'unlisted'${keep};`;
  return ["-- Written by pnpm assets:publish.", ...upserts, unlist, ""].join("\n");
}

/** How Prisma's D1 adapter stores a DateTime, so Prisma reads it back. */
function sqliteTime(date: Date): string {
  return date.toISOString().replace("Z", "+00:00");
}

function sqlValue(value: string | number): string {
  return typeof value === "number" ? String(value) : sqlString(value);
}

function sqlString(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

/** The bucket bound as `binding` in wrangler.jsonc's text. */
export function bucketName(wranglerJsonc: string, binding = "BLOBS"): string {
  const config = JSON.parse(withoutJsoncExtras(wranglerJsonc)) as {
    r2_buckets?: { binding?: string; bucket_name?: string }[];
  };
  const bucket = config.r2_buckets?.find((entry) => entry.binding === binding);
  if (!bucket?.bucket_name) throw new Error(`wrangler.jsonc has no R2 bucket bound as ${binding}`);
  return bucket.bucket_name;
}

/** Drops comments and trailing commas, leaving strings alone. */
function withoutJsoncExtras(text: string): string {
  let out = "";
  for (let index = 0; index < text.length; index++) {
    const char = text[index]!;
    if (char === '"') {
      const start = index;
      for (index++; index < text.length && text[index] !== '"'; index++) {
        if (text[index] === "\\") index++;
      }
      out += text.slice(start, index + 1);
    } else if (char === "/" && text[index + 1] === "/") {
      while (index < text.length && text[index] !== "\n") index++;
      out += "\n";
    } else if (char === "/" && text[index + 1] === "*") {
      const end = text.indexOf("*/", index + 2);
      index = end === -1 ? text.length : end + 1;
    } else {
      out += char;
    }
  }
  return out.replace(/,(\s*[}\]])/g, "$1");
}
