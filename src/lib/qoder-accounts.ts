import type { QoderBundle, QoderCredit, QoderTarget } from "./types";

export interface QoderAccountGroup {
  key: string;
  label: string;
  bundles: QoderBundle[];
  targets: Partial<Record<QoderTarget, QoderBundle>>;
}

/** Only verified identities are merged; display names are not unique. */
export function groupQoderAccounts(bundles: QoderBundle[]): QoderAccountGroup[] {
  const groups = new Map<string, QoderAccountGroup>();
  for (const bundle of bundles) {
    const key = bundle.identityKey ? `identity:${bundle.identityKey}` : `bundle:${bundle.id}`;
    const group = groups.get(key) ?? { key, label: bundle.label, bundles: [], targets: {} };
    if (group.bundles.length === 0 || bundle.capturedAt >= Math.max(...group.bundles.map((item) => item.capturedAt))) group.label = bundle.label;
    group.bundles.push(bundle);
    const previous = group.targets[bundle.target];
    if (!previous || bundle.capturedAt >= previous.capturedAt) group.targets[bundle.target] = bundle;
    groups.set(key, group);
  }
  return [...groups.values()];
}

export type QoderSortKey = "lastUsed" | "remaining" | "expiry" | "createdAt" | "name";

export function qoderExpiry(credit: QoderCredit | undefined, now = Date.now()): number | null {
  if (!credit?.ok) return null;
  const dates = [credit.plan && credit.plan.remaining > 0 ? credit.planExpiresAt : null,
    ...(credit.dedicated ?? []).filter(p => p.remaining > 0).map(p => p.expiresAt)]
    .filter((t): t is number => typeof t === "number" && Number.isFinite(t) && t > now);
  return dates.length ? Math.min(...dates) : null;
}

export const QODER_SORT_OPTIONS: { value: QoderSortKey; label: string }[] = [
  { value: "lastUsed", label: "最后使用时间" },
  { value: "remaining", label: "剩余积分（多→少）" },
  { value: "expiry", label: "积分到期（近→远）" },
  { value: "createdAt", label: "添加时间（新→旧）" },
  { value: "name", label: "名称 A→Z" },
];

/** Unknown balances and unused accounts remain after accounts with known values. */
export function orderQoderAccounts(groups: QoderAccountGroup[], sort: QoderSortKey, credits: Record<string, QoderCredit>): QoderAccountGroup[] {
  const value = (group: QoderAccountGroup): number | null => {
    if (sort === "expiry") return qoderExpiry(credits[group.key]);
    if (sort === "remaining") {
      const credit = credits[group.key];
      return credit?.ok && Number.isFinite(credit.remaining) ? credit.remaining : null;
    }
    const times = group.bundles.map((bundle) => sort === "lastUsed" ? bundle.lastSwitchedAt : bundle.capturedAt).filter((time): time is number => typeof time === "number" && time > 0);
    return times.length ? Math.max(...times) : null;
  };
  return groups.map((group, index) => ({ group, index })).sort((a, b) => {
    if (sort === "name") return a.group.label.localeCompare(b.group.label, "zh-Hans-CN") || a.index - b.index;
    const left = value(a.group), right = value(b.group);
    if (left === right) return a.index - b.index;
    if (left === null) return 1;
    if (right === null) return -1;
    return (sort === "expiry" ? left - right : right - left) || a.index - b.index;
  }).map(({ group }) => group);
}
