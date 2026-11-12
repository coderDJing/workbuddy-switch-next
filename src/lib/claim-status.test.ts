import { describe, expect, it } from "vitest";
import type { ClaimLedger, ClaimLedgerEntry, ClaimState } from "./api";
import {
  claimButtonHint,
  claimButtonLabel,
  claimChipHint,
  claimChipLabel,
  deriveClaimView,
  entryFor,
} from "./claim-status";

function entry(
  accountId: string,
  state: ClaimState,
  overrides: Partial<ClaimLedgerEntry> = {},
): ClaimLedgerEntry {
  return {
    day: "2026-11-12",
    platform: "workbuddy",
    variant: "cn",
    accountId,
    scope: "daily",
    state,
    counts: { total: 10, claimed: 10, claimable: 0, blocked: 0 },
    tasks: [],
    message: "",
    updatedAt: 1_762_000_000_000,
    source: "manual",
    attemptCount: 1,
    ...overrides,
  };
}

function ledger(entries: ClaimLedgerEntry[]): ClaimLedger {
  return { day: "2026-11-12", timezone: "Asia/Shanghai", entries };
}

const accounts = [{ id: "a" }, { id: "b" }, { id: "c" }];

describe("deriveClaimView", () => {
  it("stays idle when there is no memory at all", () => {
    const view = deriveClaimView(ledger([]), "workbuddy", accounts, "daily");
    expect(view.mode).toBe("idle");
    expect(view.pending).toBe(3);
    expect(claimButtonLabel(view)).toBe("领取积分");
  });

  it("reports done only when every account is settled", () => {
    const view = deriveClaimView(
      ledger([entry("a", "claimed"), entry("b", "claimed"), entry("c", "claimed")]),
      "workbuddy",
      accounts,
      "daily",
    );
    expect(view.mode).toBe("done");
    expect(view.claimed).toBe(3);
    expect(claimButtonLabel(view)).toBe("今日已领完");
  });

  it("treats blocked accounts as settled but never claims them as claimed", () => {
    const blockedOnly = deriveClaimView(
      ledger([entry("a", "blocked"), entry("b", "blocked"), entry("c", "blocked")]),
      "workbuddy",
      accounts,
      "daily",
    );
    expect(blockedOnly.mode).toBe("blocked");
    expect(blockedOnly.claimed).toBe(0);
    expect(claimButtonLabel(blockedOnly)).toBe("今日无可领取");

    const withReason = deriveClaimView(
      ledger([entry("a", "blocked", { message: "企业账号未开放个人成长任务" })]),
      "workbuddy",
      [{ id: "a" }],
      "daily",
    );
    expect(claimButtonHint(withReason, "fallback")).toContain("企业账号未开放个人成长任务");

    const mixed = deriveClaimView(
      ledger([entry("a", "claimed"), entry("b", "blocked"), entry("c", "claimed")]),
      "workbuddy",
      accounts,
      "daily",
    );
    expect(mixed.mode).toBe("done");
  });

  it("counts claimable tasks and remaining labels for a partial day", () => {
    const view = deriveClaimView(
      ledger([
        entry("a", "claimed"),
        entry("b", "partial", {
          counts: { total: 10, claimed: 8, claimable: 2, blocked: 0 },
          tasks: [
            { label: "和 AI 聊天 5 次", state: "partial" },
            { label: "夜猫子", state: "partial" },
          ],
        }),
        entry("c", "error"),
      ]),
      "workbuddy",
      accounts,
      "daily",
    );
    expect(view.mode).toBe("partial");
    expect(view.settled).toBe(1);
    expect(view.claimable).toBe(2);
    expect(view.remaining).toEqual(["和 AI 聊天 5 次", "夜猫子"]);
    expect(view.error).toBe(1);
    expect(claimButtonHint(view, "fallback")).toContain("1/3 个账号已领完");
    expect(claimButtonHint(view, "fallback")).toContain("未完成：和 AI 聊天 5 次");
  });

  it("excludes skipped accounts and filters by variant", () => {
    const cnAccounts = [
      { id: "a", variant: "cn" },
      { id: "b", variant: "cn" },
      { id: "c", variant: "cn" },
    ];
    const view = deriveClaimView(
      ledger([
        entry("a", "claimed"),
        entry("b", "skipped"),
        entry("c", "claimed", { variant: "ai" }),
      ]),
      "workbuddy",
      cnAccounts,
      "daily",
    );
    expect(view.skipped).toBe(1);
    expect(view.total).toBe(2);
    expect(view.claimed).toBe(1);
    // c 的 ai 条目不属于国内版：算「今天还没动过」。
    expect(view.pending).toBe(1);
    expect(view.mode).toBe("partial");
    // 档位不匹配的条目不会被当成 c 的记忆。
    expect(entryFor(ledger([entry("c", "claimed", { variant: "ai" })]), "workbuddy", { id: "c", variant: "cn" }, "daily")).toBeUndefined();
  });

  it("keeps unconfirmed as active so the button does not claim completion", () => {
    const view = deriveClaimView(
      ledger([entry("a", "claimed"), entry("b", "unconfirmed"), entry("c", "claimed")]),
      "workbuddy",
      accounts,
      "daily",
    );
    expect(view.mode).toBe("partial");
    expect(view.unconfirmed).toBe(1);
  });

  it("uses the newest entry when the same account has duplicates", () => {
    const view = deriveClaimView(
      ledger([
        entry("a", "partial", { updatedAt: 10 }),
        entry("a", "claimed", { updatedAt: 20 }),
      ]),
      "workbuddy",
      [{ id: "a" }],
      "daily",
    );
    expect(view.mode).toBe("done");
  });

  it("ignores other platforms and scopes", () => {
    const view = deriveClaimView(
      ledger([
        entry("a", "claimed", { platform: "minimax" }),
        entry("b", "claimed", { scope: "checkin" }),
      ]),
      "workbuddy",
      [{ id: "a" }, { id: "b" }],
      "daily",
    );
    expect(view.mode).toBe("idle");
  });
});

describe("card chip", () => {
  it("describes per-account progress", () => {
    expect(claimChipLabel(entry("a", "claimed"))).toBe("已领完");
    expect(claimChipLabel(entry("a", "blocked"))).toBe("无任务");
    expect(claimChipLabel(entry("a", "unconfirmed"))).toBe("待确认");
    expect(claimChipLabel(entry("a", "skipped"))).toBeNull();
    expect(
      claimChipLabel(
        entry("a", "partial", { counts: { total: 5, claimed: 2, claimable: 3, blocked: 0 } }),
      ),
    ).toBe("3 项待领");
  });

  it("lists counts and remaining task names in the tooltip", () => {
    const hint = claimChipHint(
      entry("a", "partial", {
        counts: { total: 4, claimed: 2, claimable: 1, blocked: 1 },
        tasks: [{ label: "和 AI 聊天 5 次", state: "partial" }],
        message: "成长任务完成 2/4",
      }),
    );
    expect(hint).toContain("已完成 2 · 可领 1 · 未达标 1");
    expect(hint).toContain("未完成：和 AI 聊天 5 次");
    expect(hint).toContain("成长任务完成 2/4");
  });
});
