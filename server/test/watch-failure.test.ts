import { expect, it } from "vitest";
import { pageChat, requireWritable, watchChat } from "../src/transcript.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

it("returns watch and paging refusals without throwing or opening a watch", async () => {
    const home = await testHome(scratch(), { models: fauxModels().models });
    const chat = { with: { hamlet: "home", agent: "mayor" } };
    expect(
        await watchChat(home, { ...chat, past: 99 }, () => {}, "open", context),
    ).toMatchObject({
        ok: false,
        error: { kind: "no-past-session", number: 99 },
    });
    expect(await pageChat(home, chat, "unknown", context)).toMatchObject({
        ok: false,
        error: { kind: "no-item", before: "unknown" },
    });
    expect(
        requireWritable({ ...chat, from: { hamlet: "home", number: 0 } }),
    ).toMatchObject({ ok: false, error: { kind: "read-only-peer" } });
});
