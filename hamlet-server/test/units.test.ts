import { describe, expect, it } from "vitest";
import { isValidPrice, platformFeeCents } from "../src/money.ts";
import { rawMemberBytes } from "../src/package/rawMember.ts";
import { pathProblem } from "../src/package/paths.ts";
import { compareVersions, parseVersion } from "../src/versionNumber.ts";

describe("platform fee", () => {
  it("is round(price * percent / 100)", () => {
    expect(platformFeeCents(900, 10)).toBe(90);
    expect(platformFeeCents(50, 10)).toBe(5);
    expect(platformFeeCents(55, 10)).toBe(6); // 5.5 rounds up
    expect(platformFeeCents(54, 10)).toBe(5); // 5.4 rounds down
    expect(platformFeeCents(999, 10)).toBe(100); // 99.9
    expect(platformFeeCents(50000, 10)).toBe(5000);
    expect(platformFeeCents(1005, 12.5)).toBe(126); // 125.625
    expect(platformFeeCents(333, 15)).toBe(50); // 49.95
    expect(platformFeeCents(900, 0)).toBe(0);
  });

  it("accepts only 0 or 50..=50000 as a price", () => {
    expect([0, 50, 900, 50000].every(isValidPrice)).toBe(true);
    expect([1, 49, 50001, -50, 10.5].some(isValidPrice)).toBe(false);
  });
});

describe("version numbers", () => {
  it("parses MAJOR.MINOR.PATCH and orders numerically", () => {
    expect(parseVersion("1.10.0")).toEqual({ major: 1, minor: 10, patch: 0 });
    expect(parseVersion("1.0")).toBeNull();
    expect(parseVersion("1.0.0-beta")).toBeNull();
    expect(parseVersion("v1.0.0")).toBeNull();
    expect(compareVersions(parseVersion("1.10.0")!, parseVersion("1.9.9")!)).toBeGreaterThan(0);
  });
});

describe("package paths", () => {
  it("allows plain relative paths only", () => {
    expect(pathProblem("workspace/skills/a/SKILL.md")).toBeNull();
    for (const bad of ["", "/abs", "../x", "a/../b", "a/./b", "a//b", "a\\b", "x".repeat(201)]) {
      expect(pathProblem(bad), bad).not.toBeNull();
    }
  });
});

describe("raw member bytes", () => {
  it("cuts out a member exactly as sent", () => {
    const body = '{ "version" : "1.0.0", "package":{"a":[1, "}\\"]"],"b":"é"} , "z":true}';
    const bytes = rawMemberBytes(new TextEncoder().encode(body), "package")!;
    expect(new TextDecoder().decode(bytes)).toBe('{"a":[1, "}\\"]"],"b":"é"}');
    expect(rawMemberBytes(new TextEncoder().encode('{"package": 12}'), "package")).toEqual(
      new TextEncoder().encode("12"),
    );
    expect(rawMemberBytes(new TextEncoder().encode('{"other": 1}'), "package")).toBeUndefined();
  });
});
