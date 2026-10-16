import { expect, it } from "vitest";
import {
    catalogueOf,
    editCatalogue,
    readList,
    refreshModels,
} from "../src/models.ts";
import { openaiFake } from "./openai-fake.ts";
import { context, scratch, testHome } from "./support.ts";

it("reads the listing forms, ids, token prices and context", () => {
    expect(
        readList({
            data: [
                {
                    id: "vendor/large",
                    context_length: 200_000,
                    pricing: { prompt: "0.000003", completion: "0.000015" },
                },
                { id: "plain" },
                { no: "id" },
            ],
        }),
    ).toEqual([
        { id: "plain", price: null, context: null },
        {
            id: "vendor/large",
            price: { input: 3, output: 15 },
            context: 200_000,
        },
    ]);
    expect(readList([{ name: "tiny" }])).toEqual([
        { id: "tiny", price: null, context: null },
    ]);
    expect(readList({ models: [{ model: "tiny" }] })).toEqual([
        { id: "tiny", price: null, context: null },
    ]);
});

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
    expect(await catalogueOf(home, context)).toEqual([
        expect.objectContaining({
            name: "local::org/large",
            priced: "assigned",
            price: { input: 0.1, output: 0.1 },
            allowed: false,
        }),
    ]);
    await editCatalogue(
        home,
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
    await refreshModels(home, context);
    expect(await catalogueOf(home, context)).toEqual([
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
