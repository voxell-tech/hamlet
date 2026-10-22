export interface VersionNumber {
  major: number;
  minor: number;
  patch: number;
}

const VERSION_PATTERN = /^(\d+)\.(\d+)\.(\d+)$/;
// Each part is stored in a 32 bit column.
const MAX_PART = 2_147_483_647;

export function parseVersion(text: string): VersionNumber | null {
  const match = VERSION_PATTERN.exec(text);
  if (!match) return null;
  const [major, minor, patch] = match.slice(1).map(Number) as [number, number, number];
  if ([major, minor, patch].some((part) => part > MAX_PART)) return null;
  return { major, minor, patch };
}

export function compareVersions(a: VersionNumber, b: VersionNumber): number {
  return a.major - b.major || a.minor - b.minor || a.patch - b.patch;
}

/** Newest first, for Prisma `orderBy`. */
export const NEWEST_VERSION_FIRST = [
  { major: "desc" as const },
  { minor: "desc" as const },
  { patch: "desc" as const },
];
