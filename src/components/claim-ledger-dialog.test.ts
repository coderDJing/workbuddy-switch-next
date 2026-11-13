import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ClaimLedgerList } from "./claim-ledger-dialog";
import type { ClaimLedgerEntry } from "@/lib/api";

/**
 * 弹窗正文的行为：把「还剩哪几项、叫什么」摊开，而不是只有一个计数。
 * 走 `renderToStaticMarkup`，与 account-card.test.ts 的做法一致（node 环境）。
 */

function entry(overrides: Partial<ClaimLedgerEntry> = {}): ClaimLedgerEntry {
  return {
    day: "2026-11-13",
    platform: "workbuddy",
    variant: "cn",
    accountId: "acc-1",
    scope: "daily",
    state: "partial",
    counts: { total: 20, claimed: 18, claimable: 2, blocked: 0 },
    tasks: [
      { label: "和 AI 聊天 5 次", state: "partial" },
      { label: "夜猫子", state: "partial" },
    ],
    message: "成长任务完成 18/20；部分任务尚未完成",
    updatedAt: Date.UTC(2026, 10, 13, 3, 38),
    source: "manual",
    attemptCount: 2,
    ...overrides,
  };
}

const html = (element: Parameters<typeof renderToStaticMarkup>[0]) => renderToStaticMarkup(element);

describe("claim ledger dialog body", () => {
  it("lists remaining task names with their own state", () => {
    const markup = html(
      createElement(ClaimLedgerList, {
        entries: [entry()],
        accountNames: { "acc-1": "核动力驴" },
      }),
    );
    expect(markup).toContain("核动力驴");
    expect(markup).toContain("和 AI 聊天 5 次");
    expect(markup).toContain("夜猫子");
    expect(markup).toContain("可领取");
    expect(markup).toContain("已完成");
    expect(markup).toContain("18");
    expect(markup).toContain("共 20 项");
    expect(markup).toContain("成长任务完成 18/20");
    expect(markup).toContain("今天第 2 次");
  });

  it("marks locked tasks separately from claimable ones", () => {
    const markup = html(
      createElement(ClaimLedgerList, {
        entries: [
          entry({
            state: "claimed",
            counts: { total: 20, claimed: 18, claimable: 0, blocked: 2 },
            tasks: [
              { label: "关注公众号", state: "blocked" },
              { label: "skill_share", state: "blocked" },
            ],
          }),
        ],
        accountNames: { "acc-1": "核动力驴" },
      }),
    );
    expect(markup).toContain("今日已领完");
    expect(markup).toContain("关注公众号");
    expect(markup).toContain("未达标");
  });

  it("lists accounts that have not run today instead of hiding them", () => {
    const markup = html(
      createElement(ClaimLedgerList, {
        entries: [entry()],
        accountNames: { "acc-1": "核动力驴" },
        pendingAccounts: [{ id: "acc-2", name: "杰" }],
      }),
    );
    expect(markup).toContain("杰");
    expect(markup).toContain("今天还没跑过");
  });

  it("explains the empty state instead of rendering a blank panel", () => {
    const markup = html(createElement(ClaimLedgerList, { entries: [] }));
    expect(markup).toContain("今天还没有领取记录");
  });
});
