import { describe, expect, it } from "vitest";
import { qoderCreditView, qoderRangeStart } from "./qoder-statistics";
import type { QoderCreditSource } from "./types";

describe("Qoder credit statistics", () => {
  const day = (key: string, credits: number) => ({ key, credits, billableCredits: credits / 2, nonBillableCredits: credits / 2, unknownBillingCredits: 0, records: 1 });
  const old = day("2026-10-31", 8), current = day("2026-11-06", 4);
  const source: QoderCreditSource = { source: "qoder", summary: current, daily: [old, current], dailyByModel: { older: [old], current: [current] }, models: [], projects: [], requests: [], filesScanned: 1, parseErrors: 0, coverageStartAt: null, coverageEndAt: null };
  const now = new Date(2026, 10, 6, 12);
  it("uses local calendar boundaries across months", () => {
    expect(qoderRangeStart("7d", now)).toBe("2026-10-31");
    expect(qoderRangeStart("today", now)).toBe("2026-11-06");
  });
  it("keeps summaries, daily charts and model rankings on the same filter", () => {
    const today = qoderCreditView(source, "today", "all", now);
    expect(today.summary.credits).toBe(4);
    expect(today.models.map((m) => m.key)).toEqual(["current"]);
    expect(today.summary.credits).toBe(today.summary.billableCredits + today.summary.nonBillableCredits + today.summary.unknownBillingCredits);
    expect(qoderCreditView(source, "7d", "older", now).summary.credits).toBe(8);
    expect(qoderCreditView(source, "today", "older", now).summary.records).toBe(0);
  });
});
