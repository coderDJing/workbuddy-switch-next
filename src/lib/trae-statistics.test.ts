import { describe, expect, it } from "vitest";
import { observedTraeUsage } from "./trae-statistics";
import type { TraeSnapshot } from "./api";
const point = (at: number, used: number, patch: Partial<TraeSnapshot["credit"]> = {}): TraeSnapshot => ({ accountId: "a", name: "A", variant: "cn", at, credit: { available: true, unlimited: false, unit: "credits", checkedAt: at, remaining: 100 - used, used, total: 100, periodEndAt: 999999, ...patch } });
describe("observed TRAE usage", () => {
  it("uses observed counters without fabricating the baseline or quota reset", () => {
    const points = [point(100, 10), point(200, 15), point(300, 1), point(400, 3), point(500, 20, { total: 200 })];
    expect(observedTraeUsage(points, 0, 1000, "credits").total).toBe(7);
    expect(observedTraeUsage(points, 0, 1000, "credits").observations).toBe(2);
    expect(observedTraeUsage([points[0]], 0, 1000, "credits").total).toBeNull();
  });
  it("keeps currency changes, unknown periods and missing data out of totals", () => {
    expect(observedTraeUsage([point(100, 10), point(200, 20, { unit: "usd" }), point(300, 30)], 0, 1000, "credits").total).toBeNull();
    expect(observedTraeUsage([point(100, 10, { periodEndAt: undefined }), point(200, 20, { periodEndAt: undefined })], 0, 1000, "credits").total).toBeNull();
    expect(observedTraeUsage([point(100, 10), point(200, 20)], 150, 1000, "credits").total).toBeNull();
    expect(observedTraeUsage([point(100, 10), point(200, 20, { available: false })], 0, 1000, "credits").total).toBeNull();
  });
});
