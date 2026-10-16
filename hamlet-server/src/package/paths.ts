export const MAX_PATH_CHARS = 200;
export const NAME_PATTERN = /^[a-z0-9][a-z0-9_-]{0,31}$/;

/** Why a package file path is not allowed, or null when it is fine. */
export function pathProblem(path: string): string | null {
  if (path.length === 0) return "path is empty";
  if (path.length > MAX_PATH_CHARS) return `path is longer than ${MAX_PATH_CHARS} chars`;
  if (path.includes("\\")) return "path must use / not \\";
  if (path.startsWith("/")) return "path must be relative";
  for (const segment of path.split("/")) {
    if (segment === "") return "path has an empty segment";
    if (segment === "." || segment === "..") return `path has a "${segment}" segment`;
  }
  return null;
}

/**
 * Where a file of an agent house may go: `CHARACTER.md`,
 * `workspace/NOTES.md`, or inside `workspace/skills/<skill>/`.
 */
export function agentPathProblem(path: string): string | null {
  if (path === "CHARACTER.md" || path === "workspace/NOTES.md") return null;
  const segments = path.split("/");
  if (segments.length >= 4 && segments[0] === "workspace" && segments[1] === "skills") {
    return NAME_PATTERN.test(segments[2]!)
      ? null
      : `skill folder "${segments[2]}" must match ${NAME_PATTERN.source}`;
  }
  return `"${path}" is not allowed; only CHARACTER.md, workspace/NOTES.md and workspace/skills/<skill>/...`;
}

/** Skill folder names directly under `workspace/skills/`, sorted. */
export function skillNames(paths: string[]): string[] {
  const names = new Set<string>();
  for (const path of paths) {
    const segments = path.split("/");
    if (segments.length >= 4 && segments[0] === "workspace" && segments[1] === "skills") {
      names.add(segments[2]!);
    }
  }
  return [...names].sort();
}
