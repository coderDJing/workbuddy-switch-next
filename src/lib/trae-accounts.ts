import type { TraeAccount, TraeClient, TraeStatus } from "./api";

export interface TraeAccountGroup {
  key: string;
  name: string;
  accounts: TraeAccount[];
  targets: Partial<Record<TraeClient, TraeAccount>>;
}

/** Identify regional users by verified user IDs; never merge nicknames. */
export function groupTraeAccounts(accounts: TraeAccount[]): TraeAccountGroup[] {
  const groups = new Map<string, TraeAccountGroup>();
  for (const account of accounts) {
    const key = `${account.variant}:${account.uid}`;
    const group = groups.get(key) ?? { key, name: account.name, accounts: [], targets: {} };
    if (!group.accounts.length || account.capturedAt >= Math.max(...group.accounts.map(a => a.capturedAt))) group.name = account.name;
    group.accounts.push(account);
    const previous = group.targets[account.client];
    if (!previous || account.capturedAt >= previous.capturedAt) group.targets[account.client] = account;
    groups.set(key, group);
  }
  return [...groups.values()];
}

export type TraeSortKey = "active" | "createdAt" | "name";
export const TRAE_SORT_OPTIONS: { value: TraeSortKey; label: string }[] = [
  { value: "active", label: "当前使用优先" },
  { value: "createdAt", label: "添加时间（新→旧）" },
  { value: "name", label: "名称 A→Z" },
];
export function orderTraeAccounts(groups: TraeAccountGroup[], sort: TraeSortKey, clients: TraeStatus["clients"]): TraeAccountGroup[] {
  const active = (group: TraeAccountGroup) => group.accounts.some(a => clients.some(c => c.variant === a.variant && c.client === a.client && c.activeAccountId === a.id));
  return [...groups].sort((a, b) => {
    if (sort === "active" && active(a) !== active(b)) return Number(active(b)) - Number(active(a));
    if (sort !== "name") {
      const time = Math.max(...b.accounts.map(a => a.capturedAt)) - Math.max(...a.accounts.map(a => a.capturedAt));
      if (time) return time;
    }
    return a.name.localeCompare(b.name, "zh-CN") || a.key.localeCompare(b.key);
  });
}
