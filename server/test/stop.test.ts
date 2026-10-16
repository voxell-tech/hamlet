import {
    fauxAssistantMessage,
    fauxToolCall,
} from "@earendil-works/pi-ai/providers/faux";
import { expect, it, vi } from "vitest";
import { agentKey } from "../src/hamlet.ts";
import { InstructionsDoc, owed } from "../src/instructions.ts";
import { followHome, stopAgent } from "../src/talk.ts";
import { localDay } from "../src/time.ts";
import { OversightDayDoc, settle } from "../src/waits.ts";
import {
    context,
    crashing,
    fauxModels,
    MAYOR,
    mayorChat,
    openWaits,
    say,
    scratch,
    setFolder,
    testHome,
    transcript,
} from "./support.ts";

it("does not keep an instruction owed after a crash between its reservation and admission", async () => {
    const dir = scratch(),
        { models, faux } = fauxModels();
    const home = await testHome(dir, {
        models,
        openStorage: crashing((writes) =>
            writes.some(
                (w) => w.type === "submission" && w.value.type === "input",
            ),
        ),
    });
    await expect(say(home, "Never admitted.")).rejects.toThrow("crash");
    await home.harness.closed;
    const again = await testHome(dir, { models });
    const instructions = await again.harness.snapshot(
        InstructionsDoc,
        agentKey(MAYOR),
        context,
    );
    expect(
        owed(instructions, again.now() + 10 * 86400000, 15 * 60000),
    ).toBeUndefined();
    expect(faux.state.callCount).toBe(0);
});

it("stops a held turn, drops its queued chief line, lapses waits and spares the next line", async () => {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), {
        models,
        env: { HAMLET_REFLECT_EVERY: "0" },
    });
    await setFolder(home, scratch(), "manual");
    faux.setResponses([
        fauxAssistantMessage(fauxToolCall("bash", { command: "echo never" }), {
            stopReason: "toolUse",
        }),
        fauxAssistantMessage(
            fauxToolCall("bash", { command: "echo still gated" }),
            { stopReason: "toolUse" },
        ),
        fauxAssistantMessage("The next line answers."),
    ]);
    const first = await say(home, "Run a command.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    const queued = await say(home, "Drop this.");
    expect(await stopAgent(home, MAYOR, context)).toBe("Stopping mayor.");
    expect(await first.wait(context)).toMatchObject({ status: "unanswered" });
    expect(await queued.wait(context)).toMatchObject({ status: "unanswered" });
    expect(await openWaits(home)).toEqual([]);
    await followHome(home, context);
    const instructions = await home.harness.snapshot(
        InstructionsDoc,
        agentKey(MAYOR),
        context,
    );
    expect(
        owed(instructions, home.now() + 10 * 86400000, 15 * 60000),
    ).toBeUndefined();
    expect(
        (await transcript(await mayorChat(home))).at(-1)?.data,
    ).toMatchObject({ text: "Stopped: stopped by chief (1 queued dropped)" });
    const events = (
        await home.harness.snapshot(
            OversightDayDoc,
            localDay(home.now()),
            context,
        )
    )?.events;
    expect(events).toContainEqual(
        expect.objectContaining({ type: "settled", how: "stopped" }),
    );
    const after = await say(home, "After the stop.");
    await vi.waitFor(async () => expect(await openWaits(home)).toHaveLength(1));
    const [wait] = await openWaits(home);
    if (wait === undefined) throw new Error("missing next approval");
    await home.harness.commit(
        (tx) =>
            settle(
                tx,
                wait.id,
                { how: "deny", chief: home.chief, text: "" },
                home.now(),
            ),
        context,
    );
    expect(await after.wait(context)).toMatchObject({ status: "done" });
});
