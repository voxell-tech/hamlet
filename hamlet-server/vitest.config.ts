import { dirname, resolve } from "node:path";
import { defineConfig, type Plugin } from "vitest/config";

const WASM_PREFIX = "\0wasm-module:";

/**
 * The Prisma client is generated for workerd, which imports its query
 * compiler as `*.wasm?module`. Node cannot, so tests compile it here.
 */
function wasmModule(): Plugin {
  return {
    name: "wasm-module",
    enforce: "pre",
    resolveId(id, importer) {
      if (!id.endsWith(".wasm?module") || !importer) return null;
      return WASM_PREFIX + resolve(dirname(importer), id.slice(0, -"?module".length));
    },
    load(id) {
      if (!id.startsWith(WASM_PREFIX)) return null;
      const path = JSON.stringify(id.slice(WASM_PREFIX.length));
      return `import { readFileSync } from "node:fs";\nexport default new WebAssembly.Module(readFileSync(${path}));`;
    },
  };
}

export default defineConfig({
  plugins: [wasmModule()],
  test: {
    include: ["test/**/*.test.ts"],
    testTimeout: 20_000,
    hookTimeout: 60_000,
  },
});
