import { describe, expect, it } from "vitest";
import { groupTraeAccounts, orderTraeAccounts } from "./trae-accounts";
import type { TraeAccount } from "./api";

const account = (id: string, client: "desktop", extra: Partial<TraeAccount> = {}): TraeAccount => ({
  id, uid: "same-user", name: "同名账号", variant: "cn", client, email: null, capturedAt: 1, expiresAt: null, needsLogin: false, ...extra,
});
describe("TRAE account presentation", () => {
  it("keeps regional identities separate without merging matching nicknames", () => {
    const groups = groupTraeAccounts([
      account("desktop", "desktop"),
      account("other", "desktop", { uid: "other-user" }), account("international", "desktop", { variant: "ai" }),
    ]);
    expect(groups).toHaveLength(3);
    expect(groups[0].name).toBe("同名账号");
    expect(groups[0].targets.desktop?.id).toBe("desktop");
    expect(groups[0].accounts.map(a => a.id)).toEqual(["desktop"]);
  });
  it("prioritizes the active client without mutating the shared account list", () => {
    const groups = groupTraeAccounts([account("older", "desktop"), account("newer", "desktop", { uid: "new", capturedAt: 20 })]);
    const clients = [{ variant: "cn" as const, client: "desktop" as const, name: "Trae CN", detected: true, configured: true, running: true, activeAccountId: "older", recognitionError: null }];
    expect(orderTraeAccounts(groups, "active", clients).map(g => g.targets.desktop?.id)).toEqual(["older", "newer"]);
    expect(orderTraeAccounts(groups, "createdAt", clients).map(g => g.targets.desktop?.id)).toEqual(["newer", "older"]);
    expect(groups[0].targets.desktop?.id).toBe("older");
  });
});
