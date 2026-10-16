import { expect, it } from "vitest";
import { Credentials } from "../src/home/credentials.ts";
import { secretsFrom } from "../src/scrub.ts";
import { runConfined } from "../src/tools/run.ts";
import { context, houseEnv, scratch } from "./support.ts";

it("scrubs streamed command output and reports a timeout once with its policy", async () => {
    const { env } = houseEnv({ wallSecs: 1 });
    const credentials = await Credentials.open(`${scratch()}/credentials.json`);
    await credentials.modify("openai", async () => ({
        type: "api_key",
        key: "secret-key-012345",
    }));
    const output: string[] = [];
    const result = await runConfined(
        env,
        ["bash", "-c", "printf secret-key-012345; sleep 5"],
        {
            secrets: secretsFrom(credentials),
            wallSecs: 1,
            onOutput: (text) => output.push(text),
        },
        context,
    );
    expect(output.join("")).toBe("[REDACTED]\n[timed out after 1s]");
    expect(result).toMatchObject({
        exitCode: -1,
        timedOut: true,
        policy: env.policyHash,
    });
    await env.cleanup(context);
});
