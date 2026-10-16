import { expect, it } from "vitest";
import { openaiFake } from "./openai-fake.ts";
import { context, scratch, testHome } from "./support.ts";

it("keeps the chief's price, allowance and note across a refresh and restart", async () => {
    const endpoint = await openaiFake();
    const dir = scratch();
    const options = {
        env: {
            HAMLET_MODEL: "local::org/large",
            HAMLET_BASE_URL: endpoint.url,
        },
    };
    let home = await testHome(dir, options);
    expect(await home.modelHome.catalogue(context)).toEqual([
        expect.objectContaining({
            name: "local::org/large",
            priced: "assigned",
            price: { input: 0.1, output: 0.1 },
            allowed: false,
        }),
    ]);
    await home.modelHome.editCatalogue(
        "local::org/large",
        {
            price: { input: 9, output: 10 },
            note: "Hard questions.",
            allowed: true,
        },
        context,
    );
    endpoint.listing({
        data: [
            {
                id: "org/large",
                context_length: 64_000,
                pricing: { prompt: "0.000003", completion: "0.000015" },
            },
        ],
    });
    await home.modelHome.refresh(context);
    expect(await home.modelHome.catalogue(context)).toEqual([
        expect.objectContaining({
            priced: "yours",
            price: { input: 9, output: 10 },
            note: "Hard questions.",
            allowed: true,
            context: 64_000,
        }),
    ]);
    expect(home.models.getModel("local", "org/large")?.cost.input).toBe(9);
    await home.close(context);
    await endpoint.close();
    home = await testHome(dir, options);
    expect(home.models.getModel("local", "org/large")).toMatchObject({
        contextWindow: 64_000,
        cost: { input: 9, output: 10 },
    });
});
