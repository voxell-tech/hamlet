import { Buffer } from "node:buffer";
import { z } from "zod";
import { NAME_PATTERN, agentPathProblem, pathProblem } from "./paths.ts";

export const MAX_HAMLET_AGENTS = 16;
export const MAX_PLUGIN_TOOLS = 16;
const MAX_SEED = 2n ** 64n - 1n;
const BASE64_PATTERN = /^[A-Za-z0-9+/]*={0,2}$/;

export const nameSchema = z.string().regex(NAME_PATTERN, `must match ${NAME_PATTERN.source}`);

const pathSchema = z.string().superRefine((path, ctx) => {
  const problem = pathProblem(path);
  if (problem) ctx.addIssue({ code: "custom", message: problem });
});

const base64Schema = z.string().refine(isBase64, "is not valid base64");

const fileSchema = z.object({
  path: pathSchema,
  data: base64Schema,
  executable: z.boolean().optional(),
});

export type PackageFile = z.infer<typeof fileSchema>;

const filesSchema = z.array(fileSchema).superRefine((files, ctx) => {
  const seen = new Set<string>();
  files.forEach((file, index) => {
    if (seen.has(file.path)) {
      ctx.addIssue({
        code: "custom",
        message: `"${file.path}" appears twice`,
        path: [index, "path"],
      });
    }
    seen.add(file.path);
  });
});

const agentSchema = z.object({
  name: nameSchema,
  purpose: z.string().min(1).max(500),
  model: z.string().nullable().optional(),
  files: filesSchema.superRefine((files, ctx) => {
    files.forEach((file, index) => {
      const problem = agentPathProblem(file.path);
      if (problem) ctx.addIssue({ code: "custom", message: problem, path: [index, "path"] });
    });
  }),
});

export type AgentPackage = z.infer<typeof agentSchema>;

const seedSchema = z
  .string()
  .regex(/^\d{1,20}$/, "must be a decimal string")
  .refine((seed) => BigInt(seed) <= MAX_SEED, "must fit in a u64");

const hamletSchema = z
  .object({
    name: nameSchema,
    title: z.string().min(1).max(32),
    kind: z.string().min(1).max(60),
    purpose: z.string().min(1).max(500),
    seed: seedSchema,
    mayor: agentSchema,
    agents: z.array(agentSchema).max(MAX_HAMLET_AGENTS),
  })
  .superRefine((hamlet, ctx) => {
    const seen = new Set([hamlet.mayor.name]);
    hamlet.agents.forEach((agent, index) => {
      if (seen.has(agent.name)) {
        ctx.addIssue({
          code: "custom",
          message: `agent name "${agent.name}" is used twice`,
          path: ["agents", index, "name"],
        });
      }
      seen.add(agent.name);
    });
  });

const toolSchema = z.object({ name: nameSchema, files: filesSchema }).superRefine((tool, ctx) => {
  const problem = manifestProblem(tool.name, tool.files);
  if (problem) ctx.addIssue({ code: "custom", message: problem, path: ["files"] });
});

const pluginSchema = z.object({
  tools: z
    .array(toolSchema)
    .min(1)
    .max(MAX_PLUGIN_TOOLS)
    .superRefine((tools, ctx) => {
      const seen = new Set<string>();
      tools.forEach((tool, index) => {
        if (seen.has(tool.name)) {
          ctx.addIssue({
            code: "custom",
            message: `tool name "${tool.name}" is used twice`,
            path: [index, "name"],
          });
        }
        seen.add(tool.name);
      });
    }),
});

export const packageSchema = z.discriminatedUnion("kind", [
  z.object({ format: z.literal(1), kind: z.literal("agent"), agent: agentSchema }),
  z.object({ format: z.literal(1), kind: z.literal("hamlet"), hamlet: hamletSchema }),
  z.object({ format: z.literal(1), kind: z.literal("plugin"), plugin: pluginSchema }),
]);

export type Package = z.infer<typeof packageSchema>;
export type HamletPackage = Extract<Package, { kind: "hamlet" }>["hamlet"];
export type PluginPackage = Extract<Package, { kind: "plugin" }>["plugin"];

export function decodeFile(file: PackageFile): string {
  return Buffer.from(file.data, "base64").toString("utf8");
}

/** The parsed `manifest.json` of a plugin tool, or null when absent or not an object. */
export function readManifest(files: PackageFile[]): Record<string, unknown> | null {
  const file = files.find((candidate) => candidate.path === "manifest.json");
  if (!file) return null;
  try {
    const value: unknown = JSON.parse(decodeFile(file));
    return isObject(value) ? value : null;
  } catch {
    return null;
  }
}

function manifestProblem(toolName: string, files: PackageFile[]): string | null {
  if (!files.some((file) => file.path === "manifest.json")) {
    return "must include manifest.json";
  }
  const manifest = readManifest(files);
  if (!manifest) return "manifest.json must be a JSON object";
  if (manifest.name !== toolName) {
    return `manifest.json name must be "${toolName}"`;
  }
  const exec = manifest.exec;
  if (
    !Array.isArray(exec) ||
    exec.length === 0 ||
    !exec.every((part) => typeof part === "string")
  ) {
    return "manifest.json exec must be a non-empty array of strings";
  }
  if (manifest.description !== undefined && typeof manifest.description !== "string") {
    return "manifest.json description must be a string";
  }
  return null;
}

function isBase64(data: string): boolean {
  return data.length % 4 === 0 && BASE64_PATTERN.test(data);
}

export function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
