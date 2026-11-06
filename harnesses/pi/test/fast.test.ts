import type { Api, Model, StreamOptions } from "@earendil-works/pi-ai";
import { expect, test } from "vitest";
import { withFast } from "../src/models.ts";

const model = {} as Model<Api>;

test("the fast tier is asked for through the request of an API that has one", async () => {
    const options: StreamOptions = {
        onPayload: (payload) => ({ ...(payload as object), seen: true }),
    };
    const fast = withFast(options, true, "openai-responses");
    expect(await fast.onPayload?.({ input: "hi" }, model)).toEqual({
        input: "hi",
        seen: true,
        service_tier: "priority",
    });
    expect(withFast(options, false, "openai-responses")).toBe(options);
    expect(withFast(options, true, "anthropic-messages")).toBe(options);
});
