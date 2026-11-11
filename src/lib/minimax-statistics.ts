import type { MinimaxSnapshot } from "./api";

function period(snapshot: MinimaxSnapshot): string | null {
  const c = snapshot.credit;
  if (!c.available || c.error || c.total == null || c.used == null || !Number.isFinite(c.used) || c.used < 0 || !c.packs?.length) return null;
  // A grant, expiry, or a disappearing package starts a new baseline.
  return JSON.stringify([c.total, c.packs.map(p => [p.total, p.expiresAt ?? null]).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))]);
}
export function observedMinimaxUsage(snapshots: MinimaxSnapshot[], start: number, end: number) {
  const previous = new Map<string, MinimaxSnapshot>(), days = new Map<string, number>();
  let observations = 0;
  for (const current of [...snapshots].sort((a, b) => a.at - b.at)) {
    if (current.at > end) continue;
    const before = previous.get(current.accountId); previous.set(current.accountId, current);
    if (!before || before.at < start || current.at < start || current.at <= before.at) continue;
    const key = period(current);
    if (!key || key !== period(before) || current.credit.used! < before.credit.used!) continue;
    const day = new Intl.DateTimeFormat("sv-SE").format(current.at * 1000);
    days.set(day, (days.get(day) ?? 0) + current.credit.used! - before.credit.used!); observations += 1;
  }
  return { total: observations ? [...days.values()].reduce((s, n) => s + n, 0) : null, observations, daily: [...days].map(([date, consumed]) => ({ date, consumed })).sort((a, b) => a.date.localeCompare(b.date)) };
}
