import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { Credentials } from "../src/home/credentials.ts";
import { context, fauxModels, scratch, testHome } from "./support.ts";

describe("the credentials file", () => {
    it("is made readable by its owner alone, and keeps its connection token", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const home = await testHome(dir, { models });
        const file = join(dir, "credentials.json");
        const token = home.credentials.token;

        expect(statSync(file).mode & 0o777).toBe(0o600);
        expect(JSON.parse(readFileSync(file, "utf8"))).toEqual({
            installationId: expect.any(String),
            connection: { token },
            models: {},
            git: {},
        });
        await home.close(context);
        expect((await testHome(dir, { models })).credentials.token).toBe(token);
    });

    it("is rewritten whole, so a write cut off leaves the old file as it was", async () => {
        const dir = scratch();
        const file = join(dir, "credentials.json");
        const credentials = await Credentials.open(file);
        await credentials.modify("openai", async () => ({
            type: "api_key",
            key: "sk-first",
        }));
        const before = readFileSync(file, "utf8");
        const inode = statSync(file).ino;

        await credentials.modify("openai", async () => ({
            type: "api_key",
            key: "sk-second",
        }));

        expect(statSync(file).ino).not.toBe(inode);
        expect(statSync(file).mode & 0o777).toBe(0o600);
        expect(readdirSync(dir)).toEqual(["credentials.json"]);
        // A write cut off before its rename leaves half a file beside it.
        writeFileSync(`${file}.1.tmp`, before.slice(0, 20));
        const again = await Credentials.open(file);
        expect(await again.read("openai")).toEqual({
            type: "api_key",
            key: "sk-second",
        });
    });

    it("lists every credential value as a secret", async () => {
        const credentials = await Credentials.open(
            join(scratch(), "credentials.json"),
        );
        await credentials.modify("openai", async () => ({
            type: "api_key",
            key: "sk-key",
        }));
        await credentials.modify("chatgpt", async () => ({
            type: "oauth",
            access: "access-token",
            refresh: "refresh-token",
            expires: 1,
        }));

        expect(credentials.secrets().sort()).toEqual(
            [
                credentials.token,
                "sk-key",
                "access-token",
                "refresh-token",
            ].sort(),
        );
        await credentials.delete("openai");
        expect(credentials.secrets()).not.toContain("sk-key");
        expect(await credentials.list()).toEqual([
            { providerId: "chatgpt", type: "oauth" },
        ]);
    });

    it("lists HAMLET_API_KEY as a secret too, and never writes it", async () => {
        const dir = scratch();
        const { models } = fauxModels();
        const home = await testHome(dir, {
            models,
            env: { HAMLET_API_KEY: "sk-environment" },
        });

        expect(home.credentials.secrets()).toContain("sk-environment");
        expect(
            readFileSync(join(dir, "credentials.json"), "utf8"),
        ).not.toContain("sk-environment");
    });
});
