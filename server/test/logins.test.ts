import { readFile } from "node:fs/promises";
import type { OAuthAuth } from "@earendil-works/pi-ai";
import { describe, expect, it, vi } from "vitest";
import { openaiFake } from "./openai-fake.ts";
import { context, scratch, testHome } from "./support.ts";

describe("the home's sign-ins", () => {
    it("keeps an OpenAI key beside Hamlet's stable ChatGPT identity", async () => {
        const seen: { agent: string | undefined; id: string | undefined }[] =
            [];
        const oauth: OAuthAuth = {
            name: "ChatGPT",
            isSubscription: true,
            async login(_interaction, options) {
                seen.push({
                    agent: options?.agentName,
                    id: options?.getDeviceId?.(),
                });
                return {
                    type: "oauth",
                    access: "signed-in-access",
                    refresh: "signed-in-refresh",
                    expires: Date.now() + 3_600_000,
                };
            },
            refresh: async (credential) => credential,
            toAuth: async (credential) => ({ apiKey: credential.access }),
        };
        const endpoint = await openaiFake();
        const dir = scratch();
        let home = await testHome(dir, {
            logins: { chatgpt: oauth },
            env: {
                HAMLET_MODEL: "local::org/large",
                HAMLET_BASE_URL: endpoint.url,
            },
        });
        vi.stubEnv("OPENAI_API_KEY", "ambient-key");
        try {
            expect(await home.models.checkAuth("openai")).toBeUndefined();
        } finally {
            vi.unstubAllEnvs();
        }
        await home.modelHome.login("openai", "key", {
            prompt: async (prompt) => {
                expect(prompt.type).toBe("secret");
                return "openai-key";
            },
            notify: () => {},
        });
        await home.modelHome.login("chatgpt", "subscription", {
            prompt: async () => "",
            notify: () => {},
        });
        await home.close(context);
        home = await testHome(dir, {
            logins: { chatgpt: oauth },
            env: {
                HAMLET_MODEL: "local::org/large",
                HAMLET_BASE_URL: endpoint.url,
            },
        });
        await home.modelHome.login("chatgpt", "subscription", {
            prompt: async () => "",
            notify: () => {},
        });
        expect(seen).toEqual([
            { agent: "Hamlet", id: home.credentials.installationId },
            { agent: "Hamlet", id: home.credentials.installationId },
        ]);
        expect(await home.credentials.read("openai")).toMatchObject({
            type: "api_key",
            key: "openai-key",
        });
        expect(await home.credentials.read("chatgpt")).toMatchObject({
            type: "oauth",
            access: "signed-in-access",
        });
        const storage = await readFile(home.layout.storage);
        expect(storage.includes(Buffer.from("openai-key"))).toBe(false);
        expect(storage.includes(Buffer.from("signed-in-access"))).toBe(false);
        await expect(
            home.modelHome.login("anthropic", "subscription", {
                prompt: async () => "",
                notify: () => {},
            }),
        ).resolves.toMatchObject({
            ok: false,
            error: { kind: "method-not-offered" },
        });
    });
});
