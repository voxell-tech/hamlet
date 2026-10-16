import type { Config } from "./services.ts";

export interface ConfigVars {
  PLATFORM_FEE_PERCENT?: string;
  MAX_PACKAGE_BYTES?: string;
  PUBLIC_URL?: string;
  ADMIN_GITHUB_LOGINS?: string;
}

export const DEFAULT_CONFIG: Config = {
  platformFeePercent: 10,
  maxPackageBytes: 10_485_760,
  publicUrl: "http://localhost:8787",
  adminLogins: [],
};

export function parseConfig(vars: ConfigVars): Config {
  return {
    platformFeePercent: parseNumber(
      vars.PLATFORM_FEE_PERCENT,
      DEFAULT_CONFIG.platformFeePercent,
      "PLATFORM_FEE_PERCENT",
    ),
    maxPackageBytes: parseNumber(
      vars.MAX_PACKAGE_BYTES,
      DEFAULT_CONFIG.maxPackageBytes,
      "MAX_PACKAGE_BYTES",
    ),
    publicUrl: (vars.PUBLIC_URL || DEFAULT_CONFIG.publicUrl).replace(/\/+$/, ""),
    adminLogins: parseLogins(vars.ADMIN_GITHUB_LOGINS ?? ""),
  };
}

function parseNumber(raw: string | undefined, fallback: number, name: string): number {
  if (raw === undefined || raw.trim() === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < 0) {
    throw new Error(`${name} must be a non-negative number, got "${raw}"`);
  }
  return value;
}

function parseLogins(raw: string): string[] {
  return raw
    .split(",")
    .map((login) => login.trim().toLowerCase())
    .filter((login) => login.length > 0);
}
