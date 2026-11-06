import type { QoderCreditGroup, QoderCreditSource, QoderCreditTotals } from "./types";

export type QoderStatsRange = "today" | "7d" | "30d" | "all";
export const localDateKey = (date: Date): string => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;

export function qoderRangeStart(range: QoderStatsRange, now = new Date()): string {
  if (range === "all") return "";
  const date = new Date(now);
  date.setDate(date.getDate() - (range === "7d" ? 6 : range === "30d" ? 29 : 0));
  return localDateKey(date);
}

export function sumQoderCredits(rows: QoderCreditTotals[]): QoderCreditTotals {
  return rows.reduce((sum, row) => ({
    credits: sum.credits + row.credits,
    billableCredits: sum.billableCredits + row.billableCredits,
    nonBillableCredits: sum.nonBillableCredits + row.nonBillableCredits,
    unknownBillingCredits: sum.unknownBillingCredits + row.unknownBillingCredits,
    records: sum.records + row.records,
  }), { credits: 0, billableCredits: 0, nonBillableCredits: 0, unknownBillingCredits: 0, records: 0 });
}

/** Trends, summary and ranking use exactly the same local-calendar range. */
export function qoderCreditView(source: QoderCreditSource, range: QoderStatsRange, model = "all", now = new Date()) {
  const start = qoderRangeStart(range, now), end = localDateKey(now);
  const inRange = (row: QoderCreditGroup) => row.key >= start && row.key <= end;
  const daily = (model === "all" ? source.daily : source.dailyByModel[model] ?? []).filter(inRange);
  const models = Object.entries(source.dailyByModel)
    .filter(([key]) => model === "all" || key === model)
    .map(([key, rows]) => ({ key, ...sumQoderCredits(rows.filter(inRange)) }))
    .filter((row) => row.records > 0).sort((a, b) => b.credits - a.credits);
  const requests = source.requests.filter((r) => {
    const day = localDateKey(new Date(r.timestamp));
    return day >= start && day <= end && (model === "all" || r.model === model);
  });
  return { daily, summary: sumQoderCredits(daily), models, requests };
}
