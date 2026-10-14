import { expect, it } from "vitest";
import { addEndpoint, endpointName, isLocal } from "../src/endpoints.ts";

it("knows this machine without confusing a private network for localhost", () => {
    for (const address of [
        "localhost:11434/v1",
        "http://127.3.4.5:8080",
        "http://[::1]:1234/v1",
        "0.0.0.0:8080",
    ])
        expect(isLocal(address)).toBe(true);
    for (const address of [
        "https://openrouter.ai/api/v1",
        "http://10.0.0.2:11434",
        "192.168.1.20",
        "",
    ])
        expect(isLocal(address)).toBe(false);
});

it("keeps an address's name and numbers taken names", () => {
    const known = [{ name: "local", baseUrl: "http://localhost:11434/v1" }];
    expect(
        addEndpoint(known, {
            name: "custom",
            baseUrl: "http://localhost:11434/v1/",
        }),
    ).toEqual(known[0]);
    expect(
        addEndpoint(known, {
            name: "local",
            baseUrl: "http://localhost:1234/v1",
        }),
    ).toEqual({ name: "local-2", baseUrl: "http://localhost:1234/v1" });
    for (const name of [
        "local",
        "lm-studio",
        "custom",
        "openrouter",
        "openai",
        "google",
        "groq",
        "deepseek",
    ])
        expect(endpointName(name)).toBe(true);
    expect(endpointName("Not An Endpoint")).toBe(false);
});
