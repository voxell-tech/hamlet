import { skillNames } from "./paths.ts";
import {
  type AgentPackage,
  type HamletPackage,
  type Package,
  type PluginPackage,
  decodeFile,
  isObject,
  readManifest,
} from "./schema.ts";

const AGENT_CHARACTER_CHARS = 4000;
const SETTLER_CHARACTER_CHARS = 1000;

export interface Settler {
  name: string;
  purpose: string;
  model: string | null;
  character: string;
  skills: string[];
  notes: boolean;
}

export type AgentMeta = Settler;

export interface HamletMeta {
  name: string;
  title: string;
  kind: string;
  purpose: string;
  seed: string;
  mayor: Settler;
  agents: Settler[];
}

export interface PluginMeta {
  tools: { name: string; description: string; permissions: string[] }[];
}

export type PackageMeta = AgentMeta | HamletMeta | PluginMeta;

/** What the store shows about a package, read from the package itself. */
export function deriveMeta(pkg: Package): PackageMeta {
  switch (pkg.kind) {
    case "agent":
      return settler(pkg.agent, AGENT_CHARACTER_CHARS);
    case "hamlet":
      return hamletMeta(pkg.hamlet);
    case "plugin":
      return pluginMeta(pkg.plugin);
  }
}

function settler(agent: AgentPackage, characterChars: number): Settler {
  const paths = agent.files.map((file) => file.path);
  const character = agent.files.find((file) => file.path === "CHARACTER.md");
  return {
    name: agent.name,
    purpose: agent.purpose,
    model: agent.model ?? null,
    character: character ? cutChars(decodeFile(character), characterChars) : "",
    skills: skillNames(paths),
    notes: paths.includes("workspace/NOTES.md"),
  };
}

function hamletMeta(hamlet: HamletPackage): HamletMeta {
  return {
    name: hamlet.name,
    title: hamlet.title,
    kind: hamlet.kind,
    purpose: hamlet.purpose,
    seed: hamlet.seed,
    mayor: settler(hamlet.mayor, SETTLER_CHARACTER_CHARS),
    agents: hamlet.agents.map((agent) => settler(agent, SETTLER_CHARACTER_CHARS)),
  };
}

function pluginMeta(plugin: PluginPackage): PluginMeta {
  return {
    tools: plugin.tools.map((tool) => {
      const manifest = readManifest(tool.files) ?? {};
      return {
        name: tool.name,
        description: typeof manifest.description === "string" ? manifest.description : "",
        permissions: permissions(manifest),
      };
    }),
  };
}

function permissions(manifest: Record<string, unknown>): string[] {
  const sandbox = isObject(manifest.sandbox) ? manifest.sandbox : {};
  const granted = ["run_program"];
  if (sandbox.net === true) granted.push("network");
  if (isNonEmptyArray(sandbox.fs_rw)) granted.push("write_files");
  if (isNonEmptyArray(sandbox.fs_ro)) granted.push("read_files");
  return granted;
}

function isNonEmptyArray(value: unknown): boolean {
  return Array.isArray(value) && value.length > 0;
}

/** Cuts by characters (code points), not UTF-16 units, so no emoji is split. */
function cutChars(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length <= max ? text : chars.slice(0, max).join("");
}
