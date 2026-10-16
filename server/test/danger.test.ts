import { describe, expect, it } from "vitest";
import { judge, type Reach, type Standing } from "../src/gate.ts";

const BOXED: Standing = {
    confined: true,
    net: false,
    unconfined: false,
    asksEdits: false,
    asksCommands: false,
};
const shell = (command: string, net = false): Reach => ({
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
        expect(judge({ kind: "custom", net: true }, BOXED)).toEqual({
            ask: "network",
        });
        expect(judge({ kind: "custom", net: false }, BOXED)).toEqual({
            run: { net: false },
        });
    });
    it("asks for edits and commands separately in manual mode, with the network first (danger.rs:401)", () => {
        const manual = { ...BOXED, asksEdits: true, asksCommands: true };
        expect(judge({ kind: "edit" }, manual)).toEqual({ ask: "edit" });
        expect(judge(shell("ls"), manual)).toEqual({ ask: "command" });
        expect(judge({ kind: "custom", net: false }, manual)).toEqual({
            ask: "command",
        });
        expect(judge(shell("curl x.test"), manual)).toEqual({ ask: "network" });
        expect(
            judge({ kind: "edit" }, { ...manual, asksEdits: false }),
        ).toEqual({ run: { net: false } });
        expect(judge(shell("ls"), { ...manual, asksEdits: false })).toEqual({
            ask: "command",
        });
        for (const kind of ["read", "other"] as const) {
            expect(judge({ kind }, { ...manual, net: true })).toEqual({
                run: { net: false },
            });
        }
    });
    it("holds every unconfined command until permitted, but still holds destructive ones (danger.rs:356)", () => {
        const bare = { ...BOXED, confined: false };
        expect(judge(shell("ls"), bare)).toEqual({ ask: "unconfined" });
        expect(judge({ kind: "custom", net: false }, bare)).toEqual({
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
        expect(judge({ kind: "custom", net: true }, trusted)).toEqual({
            run: { net: false },
        });
    });
});
