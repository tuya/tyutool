import { describe, expect, it } from "vitest";
import { createPlatform } from "./platform";

// In the node test env `window` is undefined, so getRuntime() === 'web' and
// createPlatform() returns the WebPlatform.
describe("WebPlatform", () => {
  it("getWsUrl returns an empty string", () => {
    const p = createPlatform();
    expect(p.getWsUrl()).toBe("");
  });

  it("pickFile resolves to null to signal the DOM <input> fallback", async () => {
    const p = createPlatform();
    await expect(p.pickFile("req-1", ".bin,.hex")).resolves.toBeNull();
  });
});
