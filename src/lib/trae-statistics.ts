import type { TraeSnapshot } from "./api";

export function traeDate(seconds: number): string {
  return new Intl.DateTimeFormat("sv-SE", { timeZone: "Asia/Shanghai", year: "numeric", month: "2-digit", day: "2-digit" }).format(seconds * 1000);
}
/** Only compare observed counters in an unchanged quota period and currency. */
export function observedTraeUsage(snapshots: TraeSnapshot[], start: number, end: number, unit: "credits" | "usd") {
  const previous = new Map<string, TraeSnapshot>();
  const days = new Map<string, number>();
  let observations = 0;
  for (const current of [...snapshots].sort((a, b) => a.at - b.at)) {
    if (current.at > end) continue;
    const before = previous.get(current.accountId);
    previous.set(current.accountId, current);
    const c = current.credit, p = before?.credit;
    if (!before || before.at < start || current.at < start || current.at <= before.at || !c.available || !p?.available || c.unlimited || p.unlimited || c.unit !== unit || p.unit !== unit
      || c.total == null || c.total !== p.total || (c.periodEndAt ?? c.expiresAt) == null || (c.periodEndAt ?? c.expiresAt) !== (p.periodEndAt ?? p.expiresAt)
      || c.used == null || p.used == null || c.used < p.used) continue;
    const day = traeDate(current.at);
    days.set(day, (days.get(day) ?? 0) + c.used - p.used);
    observations += 1;
  }
  return { total: observations ? [...days.values()].reduce((sum, v) => sum + v, 0) : null, observations, daily: [...days].map(([date, consumed]) => ({ date, consumed })).sort((a, b) => a.date.localeCompare(b.date)) };
}
