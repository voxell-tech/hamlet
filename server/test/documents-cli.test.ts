import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, it } from "vitest";
import { command as hamlet, server, tty } from "./cli-support.ts";
import { scratch } from "./support.ts";

function editor(code: string) {
    const dir = scratch();
    const script = join(dir, "edit.mjs");
    const seen = join(dir, "seen.txt");
    const opened = join(dir, "opened.json");
    writeFileSync(
        script,
        `import { readFileSync, statSync, writeFileSync } from "node:fs";\nconst path = process.argv[2];\nconst text = readFileSync(path, "utf8");\nwriteFileSync(${JSON.stringify(seen)}, text);\nwriteFileSync(${JSON.stringify(opened)}, JSON.stringify({path, mode: statSync(path).mode & 0o777}));\n${code}\n`,
    );
    return { EDITOR: `${process.execPath} ${script}`, seen, opened };
}

it("edits the current whole limits in EDITOR and reports a refusal without changing them", async () => {
    const home = scratch();
    await server(home, { responses: [] });
    const good = editor(
        "const doc = JSON.parse(text); doc.budget.cost = 8; writeFileSync(path, JSON.stringify(doc));",
    );
    const saved = hamlet(["edit", "limits"], home, "", good);
    expect(await saved.ended).toBe(0);
    expect(JSON.parse(readFileSync(good.seen, "utf8"))).toMatchObject({
        budget: { cost: 5, period: "day" },
        task: { cost: 1, messages: 12 },
    });
    expect(saved.err()).toContain("Saved.");
    const bad = editor(
        "const doc = JSON.parse(text); doc.budget.cost = -1; writeFileSync(path, JSON.stringify(doc));",
    );
    const refused = hamlet(["edit", "limits"], home, "", bad);
    expect(await refused.ended).toBe(1);
    expect(refused.err()).toContain(
        "budget.cost: a cost is a number of zero or more",
    );
    expect(JSON.parse(readFileSync(bad.seen, "utf8"))).toMatchObject({
        budget: { cost: 8 },
    });
    const report = hamlet(["chat"], home, "/budget\n");
    expect(await report.ended).toBe(0);
    expect(report.err()).toContain("Hamlet today: $0.00 of $8.00");
});

it("opens the mayor's current notes in EDITOR and sends back their text", async () => {
    const home = scratch();
    await server(home, { responses: [] });
    const first = editor('writeFileSync(path, "# Notes\\nTea at noon.\\n");');
    const saved = hamlet(["edit", "notes"], home, "", first);
    expect(await saved.ended).toBe(0);
    expect(readFileSync(first.seen, "utf8")).toBe("# Notes\n");
    const next = editor("");
    const reread = hamlet(["edit", "notes"], home, "", next);
    expect(await reread.ended).toBe(0);
    expect(readFileSync(next.seen, "utf8")).toBe("# Notes\nTea at noon.\n");
});

it("preserves a document beyond the inspector's 6000-character view, including its whitespace", async () => {
    const home = scratch();
    await server(home, { responses: [] });
    const text = `  ${"x".repeat(7000)}\n\n`;
    const first = editor(`writeFileSync(path, ${JSON.stringify(text)});`);
    expect(await hamlet(["edit", "notes"], home, "", first).ended).toBe(0);
    const next = editor("");
    expect(await hamlet(["edit", "notes"], home, "", next).ended).toBe(0);
    expect(readFileSync(next.seen, "utf8")).toBe(text);
});

it.each(["charter", "character"] as const)(
    "saves the whole %s and reads it back for the next edit",
    async (doc) => {
        const home = scratch();
        await server(home, { responses: [] });
        const text = "  Be calm, and keep it short.\n\n";
        const first = editor(`writeFileSync(path, ${JSON.stringify(text)});`);
        expect(await hamlet(["edit", doc], home, "", first).ended).toBe(0);
        const next = editor("");
        expect(await hamlet(["edit", doc], home, "", next).ended).toBe(0);
        expect(readFileSync(next.seen, "utf8")).toBe(text);
    },
);

it("edits settings as whole JSON without credentials or the server's view fields", async () => {
    const home = scratch();
    await server(home, { responses: [] });
    const first = editor(
        "const doc = JSON.parse(text); doc.reflectEvery = 3; doc.allowNet = true; writeFileSync(path, JSON.stringify(doc));",
    );
    expect(await hamlet(["edit", "settings"], home, "", first).ended).toBe(0);
    expect(JSON.parse(readFileSync(first.seen, "utf8"))).toEqual({
        model: "faux::faux-1",
        reflectEvery: 10,
        allowNet: false,
        endpoints: [{ name: "local", baseUrl: "http://localhost:11434/v1" }],
    });
    for (const change of ["doc.allowNte = false", "doc.model = null"]) {
        const invalid = editor(
            `const doc = JSON.parse(text); doc.reflectEvery = 99; ${change}; writeFileSync(path, JSON.stringify(doc));`,
        );
        const refused = hamlet(["edit", "settings"], home, "", invalid);
        expect(await refused.ended).toBe(1);
        expect(refused.err()).toContain("nothing was saved");
    }
    const next = editor("");
    expect(await hamlet(["edit", "settings"], home, "", next).ended).toBe(0);
    expect(JSON.parse(readFileSync(next.seen, "utf8"))).toMatchObject({
        reflectEvery: 3,
        allowNet: true,
    });
}, 10_000);

it("saves nothing when EDITOR fails and removes its private temporary document", async () => {
    const home = scratch();
    await server(home, { responses: [] });
    const failed = editor('writeFileSync(path, "not saved"); process.exit(2);');
    const run = hamlet(["edit", "notes"], home, "", failed);
    expect(await run.ended).toBe(1);
    expect(run.err()).toContain("The editor ended with 2. Nothing was saved.");
    const opened = JSON.parse(readFileSync(failed.opened, "utf8")) as {
        path: string;
        mode: number;
    };
    expect(opened.mode).toBe(0o600);
    expect(existsSync(opened.path)).toBe(false);
    const next = editor("");
    expect(await hamlet(["edit", "notes"], home, "", next).ended).toBe(0);
    expect(readFileSync(next.seen, "utf8")).toBe("# Notes\n");
});

it("gives a terminal editor the chief's terminal on stdin and stdout", async () => {
    const home = scratch();
    await server(home, { responses: [] });
    const script = join(scratch(), "terminal-editor.sh");
    writeFileSync(
        script,
        "[ -t 0 ] && echo stdin-tty || echo stdin-not-tty\n[ -t 1 ] && echo stdout-tty || echo stdout-not-tty\n",
    );
    const result = await tty(["edit", "charter"], home, [], {
        EDITOR: `/bin/sh ${script}`,
    });
    expect(result.code).toBe(0);
    expect(result.output).toContain("stdin-tty");
    expect(result.output).toContain("stdout-tty");
    expect(result.output).not.toContain("not-tty");
});
