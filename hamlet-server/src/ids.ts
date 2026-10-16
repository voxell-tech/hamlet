import { createHash, randomBytes } from "node:crypto";

export function newId(prefix: "u" | "s" | "l" | "v" | "p" | "r" | "ap"): string {
  return `${prefix}_${randomBytes(12).toString("hex")}`;
}

export function newSessionToken(): string {
  return `hms_${randomBytes(32).toString("hex")}`;
}

export function sha256Hex(data: string | Uint8Array): string {
  return createHash("sha256").update(data).digest("hex");
}
