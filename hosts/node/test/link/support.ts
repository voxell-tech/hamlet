import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../../../..", import.meta.url));

/** The Node host's entry. */
export const MAIN = fileURLToPath(
    new URL("../../src/main.ts", import.meta.url),
);

/** The built example `name` of the Rust workspace. */
export function example(name: string): string {
    return join(
        process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"),
        "debug",
        "examples",
        process.platform === "win32" ? `${name}.exe` : name,
    );
}
