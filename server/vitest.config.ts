import { configDefaults, defineConfig } from "vitest/config";

/** Tests that reach a model server, run only by `npm run test:live` (S14). */
const live = "test/**/*.live.test.ts";

export default defineConfig(({ mode }) => ({
    test: {
        include: [mode === "live" ? live : "test/**/*.test.ts"],
        exclude:
            mode === "live"
                ? configDefaults.exclude
                : [...configDefaults.exclude, live],
        // The linear engine runs a model's pattern, as the `hamlet` script.
        execArgv: ["--enable-experimental-regexp-engine"],
    },
}));
