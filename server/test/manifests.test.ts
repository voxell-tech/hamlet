import { expect, it } from "vitest";
import { manifestFaultText, parseManifest } from "../src/hand.ts";

it("normalizes a hand manifest once, retaining arguments, relative reach and environment values", () => {
    const result = parseManifest(
        JSON.stringify({
            name: "greet",
            exec: ["printf", 7, "hi"],
            sandbox: {
                fs_ro: ["data"],
                fs_rw: ["../out"],
                env: { LANG: "C", IGNORED: 7 },
                limits: { wall_secs: 3 },
            },
        }),
        "/tools/greet",
    );
    expect(result).toMatchObject({
        ok: true,
        value: {
            name: "greet",
            exec: ["printf", "hi"],
            ro: ["/tools/greet/data", "/tools/greet"],
            rw: ["/tools/out"],
            env: { LANG: "C", IGNORED: "" },
            wallSecs: 3,
            cpuSecs: null,
            parameters: { type: "object", properties: {} },
        },
    });
});

it("classifies manifest failures without putting private JSON in their words", () => {
    const broken = parseManifest('{"name": private-token}', "/tools/greet");
    expect(broken).toMatchObject({ ok: false, error: { kind: "not-json" } });
    if (broken.ok) throw new Error("expected failure");
    expect(manifestFaultText(broken.error)).toBe(
        "/tools/greet: manifest.json is not valid JSON",
    );
    for (const [input, kind] of [
        [{}, "missing-name"],
        [{ name: "bad.name", exec: ["echo"] }, "name-format"],
        [{ name: "good", exec: [7] }, "exec-empty"],
        [
            { name: "good", exec: ["echo"], parameters: [] },
            "parameters-not-schema",
        ],
        [
            {
                name: "good",
                exec: ["echo"],
                parameters: {
                    type: "object",
                    properties: { text: { type: "string", pattern: "[" } },
                },
            },
            "schema-compile",
        ],
        [
            {
                name: "good",
                exec: ["echo"],
                sandbox: { limits: { wall_secs: 0 } },
            },
            "limit-not-whole",
        ],
    ] as const) {
        expect(
            parseManifest(JSON.stringify(input), "/tools/greet"),
        ).toMatchObject({ ok: false, error: { kind } });
    }
});

it("keeps stored diagnostic strings readable while new faults are structured", () => {
    expect(manifestFaultText("tools/old: missing name")).toBe(
        "tools/old: missing name",
    );
    expect(
        parseManifest(
            JSON.stringify({
                name: "long",
                exec: ["echo"],
                sandbox: { limits: { wall_secs: 2147484 } },
            }),
            "/tools/long",
        ),
    ).toMatchObject({
        ok: false,
        error: { kind: "limit-not-whole", constraint: "timer" },
    });
});
