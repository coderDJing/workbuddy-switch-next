import { describe, expect, it } from "vitest";
import type { QoderBundle, QoderTarget } from "./types";
import { groupQoderAccounts, orderQoderAccounts, qoderExpiry } from "./qoder-accounts";

function bundle(id: string, target: QoderTarget, identityKey?: string, capturedAt = 1): QoderBundle {
  return { id, target, label: "same label", identityKey, capturedAt, files: [] };
}

describe("Qoder account groups", () => {
  it("prioritizes explicit unexpired positive buckets and puts unknown expiry last", () => {
    const now = Date.now();
    const credit = { ok: true as const, total: 100, remaining: 50, used: 50, plan: { total: 10, remaining: 0, used: 10 }, planExpiresAt: now + 1000, dedicated: [{ total: 50, remaining: 20, used: 30, expiresAt: now + 5000 }] };
    expect(qoderExpiry(credit, now)).toBe(now + 5000);
    expect(qoderExpiry({ ...credit, dedicated: [{ ...credit.dedicated[0], expiresAt: now - 1 }] }, now)).toBeNull();
    const groups = groupQoderAccounts([bundle("unknown", "cli"), bundle("later", "cli"), bundle("soon", "cli")]);
    expect(orderQoderAccounts(groups, "expiry", { "bundle:soon": credit, "bundle:later": { ...credit, dedicated: [{ ...credit.dedicated[0], expiresAt: now + 10000 }] } }).map(g => g.key)).toEqual(["bundle:soon", "bundle:later", "bundle:unknown"]);
  });
  it("shows one account with separate desktop and CLI targets", () => {
    const groups = groupQoderAccounts([bundle("a", "desktop", "user-1"), bundle("b", "cli", "user-1")]);
    expect(groups).toHaveLength(1);
    expect(groups[0].targets.desktop?.id).toBe("a");
    expect(groups[0].targets.cli?.id).toBe("b");
  });

  it("does not merge unverified or different identities that share a name", () => {
    expect(groupQoderAccounts([bundle("a", "desktop"), bundle("b", "cli"), bundle("c", "desktop", "other")])).toHaveLength(3);
  });

  it("keeps earlier snapshots but selects the newest for each target", () => {
    const [group] = groupQoderAccounts([bundle("old", "desktop", "one", 1), bundle("new", "desktop", "one", 2)]);
    expect(group.bundles).toHaveLength(2);
    expect(group.targets.desktop?.id).toBe("new");
  });

  it("uses the newest saved client state and selects a later reauthorization", () => {
    const vault = { ...bundle("vault", "cli", "one", 1), files: [{ name: "authorized.dpapi", sha256: "hash", size: 10 }] };
    const snapshot = bundle("updated", "cli", "one", 2);
    expect(groupQoderAccounts([vault, snapshot])[0].targets.cli?.id).toBe("updated");
    const renewed = { ...vault, id: "renewed", capturedAt: 3 };
    expect(groupQoderAccounts([vault, snapshot, renewed])[0].targets.cli?.id).toBe("renewed");
  });

  it("orders by known remaining credits without treating unavailable data as zero", () => {
    const groups = groupQoderAccounts([bundle("unknown", "desktop"), bundle("empty", "desktop"), bundle("rich", "desktop")]);
    const credits = {
      "bundle:empty": { ok: true as const, remaining: 0, total: 0, used: 0 },
      "bundle:rich": { ok: true as const, remaining: 5, total: 10, used: 5 },
    };
    expect(orderQoderAccounts(groups, "remaining", credits).map((group) => group.key)).toEqual(["bundle:rich", "bundle:empty", "bundle:unknown"]);
  });

  it("sorts by last use across each account's clients", () => {
    const groups = groupQoderAccounts([
      { ...bundle("a", "desktop", "first"), lastSwitchedAt: 10 },
      { ...bundle("b", "cli", "first"), lastSwitchedAt: 30 },
      { ...bundle("c", "desktop", "second"), lastSwitchedAt: 20 },
    ]);
    expect(orderQoderAccounts(groups, "lastUsed", {}).map((group) => group.key)).toEqual(["identity:first", "identity:second"]);
  });
});
