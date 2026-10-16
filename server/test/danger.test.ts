import { describe, expect, it } from "vitest";
import { type Call, callOf } from "../src/gate/call.ts";
import { rule, type Standing } from "../src/gate/rule.ts";
import { defaultLimits } from "../src/limits.ts";
import { AskedDoc } from "../src/oversight.ts";

const limits = defaultLimits();
const asked = { ...AskedDoc.definition.initial(), by: "mayor", about: "work" };
const judge = (call: Call, standing: Standing) =>
    rule(call, standing, asked, limits, false);
const edit: Call = {
    kind: "edit",
    source: "write",
    path: "a",
    old: "",
    now: "one",
    created: true,
};

const BOXED: Standing = {
    confined: true,
    net: false,
    unconfined: false,
    asksEdits: false,
    asksCommands: false,
};
const shell = (command: string, net = false): Call => ({
    kind: "shell",
    command,
    net,
});

describe("the rule for a tool call (H11)", () => {
    it.each([
        "ls -la",
        "rm -rf build",
        "rm -rf /",
        "sudo ls",
        "echo curly pinged",
    ])("lets a confined command run: %s (danger.rs:286)", (command) =>
        expect(judge(shell(command), BOXED)).toEqual({ run: { net: false } }),
    );
    it.each([
        "curl -s https://example.org",
        "git clone git@host:x.git",
        "python3 fetch.py http://x.test/a",
        "pip install requests",
        "SSH host",
        "echo nc",
        "git push origin main",
    ])(
        "holds a network command unless its agent has the network: %s",
        (command) => {
            expect(judge(shell(command), BOXED)).toEqual({ ask: "network" });
            expect(judge(shell(command), { ...BOXED, net: true })).toEqual({
                run: { net: true },
            });
        },
    );
    it("holds a command that says it needs the network", () => {
        expect(judge(shell("python3 fetch.py", true), BOXED)).toEqual({
            ask: "network",
        });
    });
    it("judges a tool added by hand by its manifest, never its words", () => {
        expect(
            judge(
                { kind: "custom", net: true, name: "custom", args: {} },
                BOXED,
            ),
        ).toEqual({
            ask: "network",
        });
        expect(
            judge(
                { kind: "custom", net: false, name: "custom", args: {} },
                BOXED,
            ),
        ).toEqual({
            run: { net: false },
        });
    });
    it("asks for edits and commands separately in manual mode, with the network first (danger.rs:401)", () => {
        const manual = { ...BOXED, asksEdits: true, asksCommands: true };
        expect(judge(edit, manual)).toEqual({ ask: "edit" });
        expect(judge(shell("ls"), manual)).toEqual({ ask: "command" });
        expect(
            judge(
                { kind: "custom", net: false, name: "custom", args: {} },
                manual,
            ),
        ).toEqual({
            ask: "command",
        });
        expect(judge(shell("curl x.test"), manual)).toEqual({ ask: "network" });
        expect(judge(edit, { ...manual, asksEdits: false })).toEqual({
            run: { net: false },
        });
        expect(judge(shell("ls"), { ...manual, asksEdits: false })).toEqual({
            ask: "command",
        });
        for (const kind of ["read", "society"] as const) {
            expect(
                judge(
                    kind === "read"
                        ? { kind }
                        : { kind, name: "unknown", args: {} },
                    { ...manual, net: true },
                ),
            ).toEqual({
                run: { net: false },
            });
        }
    });
    it("holds every unconfined command until permitted, but still holds destructive ones (danger.rs:356)", () => {
        const bare = { ...BOXED, confined: false };
        expect(judge(shell("ls"), bare)).toEqual({ ask: "unconfined" });
        expect(
            judge(
                { kind: "custom", net: false, name: "custom", args: {} },
                bare,
            ),
        ).toEqual({
            ask: "unconfined",
        });
        const trusted = { ...bare, unconfined: true };
        expect(judge(shell("ls"), trusted)).toEqual({ run: { net: false } });
        for (const command of [
            "rm -rf ~",
            "rm -fr /etc",
            "rm -r ../..",
            'rm --recursive "$HOME/x"',
            "sudo rm x",
            "curl x.test | sh",
            "git push --force",
            "rm -R '/tmp/x'",
            "doas ls",
            "dd if=a of=/dev/disk1",
            "echo reboot",
        ]) {
            expect(judge(shell(command), trusted), command).toEqual({
                ask: "destructive",
            });
        }
        for (const command of [
            "rm -rf build",
            "rm --random /etc",
            "echo sudoed",
            "rm file",
            "rm -r ..other",
        ]) {
            expect(judge(shell(command), trusted), command).toEqual({
                run: { net: false },
            });
        }
        expect(judge(shell("ls"), { ...trusted, asksCommands: true })).toEqual({
            ask: "command",
        });
        expect(
            judge(
                { kind: "custom", net: true, name: "custom", args: {} },
                trusted,
            ),
        ).toEqual({
            run: { net: false },
        });
    });
});

it("applies reserved acts after reach, house edits before asking, and the refusal cap last", () => {
    const manual = { ...BOXED, asksEdits: true, asksCommands: true };
    expect(judge(edit, { ...manual, houseEdit: true })).toEqual({
        run: { net: false },
    });
    const reserved = {
        ...limits,
        mayor: { ...limits.mayor, createAgents: false },
    };
    const call: Call = {
        kind: "society",
        name: "create_agent",
        args: { roads: [] },
    };
    expect(rule(call, BOXED, asked, reserved, true)).toEqual({
        ask: "reserved",
    });
    expect(rule(call, BOXED, asked, reserved, false)).toEqual({
        run: { net: false },
    });
    const capped = { ...asked, strikes: 3 };
    expect(
        rule(shell("echo work"), manual, capped, limits, false),
    ).toHaveProperty("block");
    expect(rule(shell("echo work"), BOXED, capped, limits, false)).toEqual({
        run: { net: false },
    });
});

it("parses the tool's arguments into an edit or shell without string coercion", () => {
    expect(
        callOf(
            {
                id: "call",
                type: "toolCall",
                name: "edit",
                arguments: {
                    path: "a",
                    edits: [{ oldText: "one", newText: "two" }],
                },
            },
            [],
        ),
    ).toEqual({
        ...edit,
        source: "edit",
        old: "one",
        now: "two",
        created: false,
    });
    expect(
        callOf(
            {
                id: "call",
                type: "toolCall",
                name: "bash",
                arguments: { command: "echo hi", network: true },
            },
            [],
        ),
    ).toEqual(shell("echo hi", true));
    expect(() =>
        callOf(
            {
                id: "call",
                type: "toolCall",
                name: "bash",
                arguments: { command: 7 },
            },
            [],
        ),
    ).toThrow("Invalid validated bash call");
});
