import { fauxAssistantMessage } from "@earendil-works/pi-ai/providers/faux";
import { expect, it } from "vitest";
import { record } from "../src/agents.ts";
import { RecordDoc } from "../src/record.ts";
import {
    addAgent,
    context,
    fauxModels,
    request,
    say,
    scratch,
    testHome,
} from "./support.ts";

async function history() {
    const { models, faux } = fauxModels();
    const home = await testHome(scratch(), { models, now: () => 1000 });
    await addAgent(home, "home", "reader", "Reads");
    await home.harness.commit(async (tx) => {
        const r = await tx.doc(RecordDoc, "home", "home");
        r.decisions = [
            {
                kind: "created",
                agent: 2,
                name: "first-name",
                purpose: "Works",
                by: "mayor",
                reason: "Needed",
                at: 100,
            },
            {
                kind: "restored",
                agent: 2,
                name: "last-name",
                archive: "past",
                by: "chief:chief",
                reason: "Returned",
                at: 101,
            },
            {
                kind: "moved_in",
                agent: 3,
                name: "arrival",
                hamlet: "mill",
                by: "mayor",
                reason: "Moved",
                at: 102,
            },
            {
                kind: "erased",
                agent: 2,
                name: "last-name",
                archive: "past",
                by: "chief:chief",
                reason: "",
                at: 103,
            },
            {
                kind: "erased",
                agent: 3,
                name: "arrival",
                archive: "arrival-past",
                by: "chief:chief",
                reason: "",
                at: 104,
            },
            {
                kind: "road_built",
                a: 2,
                b: 1,
                by: "mayor",
                reason: "First road",
                at: 105,
            },
            {
                kind: "road_built",
                a: 3,
                b: 1,
                by: "mayor",
                reason: "Second road",
                at: 106,
            },
        ];
    }, context);
    return { home, faux };
}

it("keeps the record's first-created name and number fallbacks after erasure", async () => {
    const { home } = await history();
    const result = await record(
        home,
        { hamlet: "home", chief: home.chief, rest: "" },
        context,
    );
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.value).toContain(
        "between `first-name` and `reader`: First road",
    );
    expect(result.value).toContain("between `3` and `reader`: Second road");
});

it("keeps agent-number fallbacks in the brief when no living or archived name remains", async () => {
    const { home, faux } = await history();
    let section = "";
    faux.setResponses([
        (prompt, _options, _state, model) => {
            section = request(prompt.messages, model).sections.hamlet ?? "";
            return fauxAssistantMessage("ok");
        },
    ]);
    await (await say(home, "Read the record")).wait(context);
    expect(section).toContain("between `agent #2` and `reader`: First road");
    expect(section).toContain("between `agent #3` and `reader`: Second road");
});
