import { Buffer } from "node:buffer";

export function file(path: string, text: string, executable?: boolean) {
  return {
    path,
    data: Buffer.from(text, "utf8").toString("base64"),
    ...(executable === undefined ? {} : { executable }),
  };
}

export function agent(name = "pr-reviewer", extraFiles: ReturnType<typeof file>[] = []) {
  return {
    name,
    purpose: "Reviews pull requests.",
    model: "anthropic/claude-sonnet-4",
    files: [
      file("CHARACTER.md", "# Reviewer\nCareful and kind."),
      file("workspace/NOTES.md", "notes"),
      file("workspace/skills/review/SKILL.md", "# Review"),
      file("workspace/skills/review/scripts/run.sh", "#!/bin/sh\necho hi", true),
      file("workspace/skills/tests/SKILL.md", "# Tests"),
      ...extraFiles,
    ],
  };
}

export function agentPackage(overrides: Record<string, unknown> = {}) {
  return { format: 1, kind: "agent", agent: { ...agent(), ...overrides } };
}

export function hamletPackage() {
  return {
    format: 1,
    kind: "hamlet",
    hamlet: {
      name: "launch-crew",
      title: "Launch Crew",
      kind: "SaaS launch team",
      purpose: "Ships a small product.",
      seed: "18446744073709551615",
      mayor: { ...agent("mayor"), model: null },
      agents: [agent("builder"), { name: "writer", purpose: "Writes docs.", files: [] }],
    },
  };
}

export function manifest(name: string, sandbox?: Record<string, unknown>, description?: string) {
  return file(
    "manifest.json",
    JSON.stringify({
      name,
      exec: ["./run"],
      ...(description ? { description } : {}),
      ...(sandbox ? { sandbox } : {}),
    }),
  );
}

export function pluginPackage() {
  return {
    format: 1,
    kind: "plugin",
    plugin: {
      tools: [
        {
          name: "calendar",
          files: [
            manifest(
              "calendar",
              { net: true, fs_rw: ["~/cal"], fs_ro: [] },
              "Reads your calendar.",
            ),
            file("run", "#!/bin/sh", true),
          ],
        },
        { name: "clock", files: [manifest("clock", { net: false, fs_ro: ["/etc/zoneinfo"] })] },
        { name: "plain", files: [manifest("plain")] },
      ],
    },
  };
}
