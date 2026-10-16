import { expect, it } from "vitest";
import { context, fauxModels, scratch, testHome } from "./support.ts";

it("opens the home when one part cannot start and still starts later parts", async () => {
    let laterStarted = false;
    const home = await testHome(scratch(), {
        models: fauxModels().models,
        parts: [
            () => ({
                async start() {
                    throw new Error("fixture start failed");
                },
            }),
            () => ({
                async start() {
                    laterStarted = true;
                },
            }),
        ],
    });
    expect(laterStarted).toBe(true);
    await home.close(context);
});
