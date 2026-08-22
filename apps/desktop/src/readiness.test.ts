import { describe, expect, it, vi } from "vitest";
import { createReadinessGate } from "./readiness";

describe("readiness gate", () => {
  it("shares one in-flight readiness request and debounces the next request", async () => {
    let resolve: ((value: string) => void) | undefined;
    const reader = vi.fn(
      () => new Promise<string>((complete) => { resolve = complete; }),
    );
    const gate = createReadinessGate<string>(5_000);

    const first = gate.request(reader);
    const second = gate.request(reader);
    expect(reader).toHaveBeenCalledTimes(1);
    expect(gate.inFlight()).toBe(true);

    resolve?.("ready");
    await expect(Promise.all([first, second])).resolves.toEqual(["ready", "ready"]);
    expect(gate.inFlight()).toBe(false);

    await expect(gate.request(reader)).resolves.toBe("ready");
    expect(reader).toHaveBeenCalledTimes(1);
  });

  it("allows an explicit re-check after invalidation without duplicating callers", async () => {
    const reader = vi.fn().mockResolvedValue("rechecked");
    const gate = createReadinessGate<string>(5_000);

    await gate.request(reader);
    gate.invalidate();
    const first = gate.request(reader, true);
    const second = gate.request(reader, true);

    await expect(Promise.all([first, second])).resolves.toEqual(["rechecked", "rechecked"]);
    expect(reader).toHaveBeenCalledTimes(2);
  });
});
