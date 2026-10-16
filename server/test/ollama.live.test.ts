import { expect, it } from "vitest";
import { HOME } from "../src/hamlet.ts";
import { localDay } from "../src/home.ts";
import { ledgerOf } from "../src/ledger.ts";
import {
    context,
    mayorChat,
    say,
    scratch,
    testHome,
    transcript,
} from "./support.ts";

it("makes one tool call against the chief's local model", async () => {
    const home = await testHome(scratch(), {
        env: {
            HAMLET_MODEL: process.env.HAMLET_MODEL ?? "local::qwen3.6:35b-a3b",
            HAMLET_BASE_URL: process.env.HAMLET_BASE_URL,
        },
    });
    expect(
        await (
            await say(
                home,
                "Use read to read NOTES.md, then answer with one word.",
            )
        ).wait(context),
    ).toMatchObject({ status: "done" });
    expect(
        (await transcript(await mayorChat(home))).some(
            (entry) => entry.kind === "pi.tool-result",
        ),
    ).toBe(true);
    const lines = await ledgerOf(home, HOME, localDay(home.now()), context);
    expect(lines).toContainEqual(
        expect.objectContaining({
            what: "model",
            host: "localhost",
            input: expect.any(Number),
            output: expect.any(Number),
        }),
    );
}, 120_000);
