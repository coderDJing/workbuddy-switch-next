import type { ClaimLedger, ClaimLedgerEntry, ClaimScope } from "./api";

/**
 * 从「今日领取记忆」派生按钮态与卡片 chip 的纯逻辑。
 *
 * 判定口径：**没有可领项就算领完**（见 docs/claim-ledger-design.md）。
 * 后端已经把 `claimable` 算好，这里只做聚合，不再二次解释任务进度。
 */

export type ClaimButtonMode = "idle" | "partial" | "done" | "blocked";

export interface ClaimAccount {
  id: string;
  /** 档位 / 区域；记忆条目 variant 为 null（如豆包）时不参与比较。 */
  variant?: string | null;
}

export interface ClaimView {
  mode: ClaimButtonMode;
  /** 参与统计的账号数（skipped 不计入）。 */
  total: number;
  /** 今日已定局：claimed + blocked。 */
  settled: number;
  claimed: number;
  blocked: number;
  partial: number;
  error: number;
  unconfirmed: number;
  /** 今天还没有任何记忆的账号。 */
  pending: number;
  skipped: number;
  /** 剩余可领任务数（所有账号之和）。 */
  claimable: number;
  /** 剩余任务名（去重，最多 3 条）。 */
  remaining: string[];
  /** 「今日无可领取」时的首个原因（企业版 / 受限 / 登录失效）。 */
  notice?: string;
  day: string | null;
  updatedAt: number | null;
}

export const EMPTY_CLAIM_VIEW: ClaimView = {
  mode: "idle",
  total: 0,
  settled: 0,
  claimed: 0,
  blocked: 0,
  partial: 0,
  error: 0,
  unconfirmed: 0,
  pending: 0,
  skipped: 0,
  claimable: 0,
  remaining: [],
  day: null,
  updatedAt: null,
};

function sameVariant(entry: ClaimLedgerEntry, account: ClaimAccount): boolean {
  if (entry.variant == null || account.variant == null) return true;
  return entry.variant === account.variant;
}

/** 取某账号在指定平台 / scope 下最新的一条记忆。 */
export function entryFor(
  ledger: ClaimLedger | null | undefined,
  platform: string,
  account: ClaimAccount,
  scope: ClaimScope,
): ClaimLedgerEntry | undefined {
  const entries = ledger?.entries ?? [];
  let found: ClaimLedgerEntry | undefined;
  for (const entry of entries) {
    if (entry.platform !== platform || entry.scope !== scope) continue;
    if (entry.accountId !== account.id) continue;
    if (!sameVariant(entry, account)) continue;
    if (!found || entry.updatedAt >= found.updatedAt) found = entry;
  }
  return found;
}

export function deriveClaimView(
  ledger: ClaimLedger | null | undefined,
  platform: string,
  accounts: ClaimAccount[],
  scope: ClaimScope,
): ClaimView {
  if (!accounts.length) return { ...EMPTY_CLAIM_VIEW, day: ledger?.day ?? null };

  let claimed = 0;
  let blocked = 0;
  let partial = 0;
  let error = 0;
  let unconfirmed = 0;
  let pending = 0;
  let skipped = 0;
  let claimable = 0;
  let updatedAt: number | null = null;
  let notice: string | undefined;
  const remaining: string[] = [];

  for (const account of accounts) {
    const entry = entryFor(ledger, platform, account, scope);
    if (!entry) {
      pending += 1;
      continue;
    }
    updatedAt = updatedAt == null ? entry.updatedAt : Math.max(updatedAt, entry.updatedAt);
    switch (entry.state) {
      case "claimed":
        claimed += 1;
        break;
      case "blocked":
        blocked += 1;
        if (!notice && entry.message) notice = entry.message;
        break;
      case "skipped":
        skipped += 1;
        break;
      case "unconfirmed":
        unconfirmed += 1;
        claimable += entry.counts.claimable;
        break;
      case "error":
        error += 1;
        claimable += entry.counts.claimable;
        break;
      default:
        partial += 1;
        claimable += entry.counts.claimable;
        break;
    }
    for (const task of entry.tasks ?? []) {
      if (remaining.length >= 3) break;
      if (task.state === "partial" && !remaining.includes(task.label)) {
        remaining.push(task.label);
      }
    }
  }

  const total = accounts.length - skipped;
  const settled = claimed + blocked;
  let mode: ClaimButtonMode;
  if (total === 0) {
    mode = "idle";
  } else if (settled === total) {
    // 一个账号都没真正领到、且全部是「今日无可领取」时，不要谎报已领完。
    mode = claimed > 0 ? "done" : "blocked";
  } else if (settled === 0 && pending === total) {
    // 今天完全没动过：保持原始按钮。
    mode = "idle";
  } else {
    mode = "partial";
  }

  return {
    mode,
    total,
    settled,
    claimed,
    blocked,
    partial,
    error,
    unconfirmed,
    pending,
    skipped,
    claimable,
    remaining,
    notice,
    day: ledger?.day ?? null,
    updatedAt,
  };
}

/** 按钮文案：与设计稿四态一致。 */
export function claimButtonLabel(view: ClaimView): string {
  switch (view.mode) {
    case "done":
      return "今日已领完";
    case "blocked":
      return "今日无可领取";
    default:
      return "领取积分";
  }
}

/** 按钮 tooltip：说清「几个账号已领完 / 还剩什么」。 */
export function claimButtonHint(view: ClaimView, fallback: string): string {
  if (view.mode === "idle") return fallback;
  const when = view.updatedAt ? ` · ${formatClock(view.updatedAt)} 核对` : "";
  if (view.mode === "done") {
    return `${view.claimed}/${view.total} 个账号已领完${when}。点击可重新核对（会再跑一遍每日任务）`;
  }
  if (view.mode === "blocked") {
    const reason = view.notice ? `：${view.notice}` : "。活动未开放、企业版或账号受限时会出现这种情况";
    return `今日暂无可领取的奖励${reason}${when}`;
  }
  const pieces = [`${view.settled}/${view.total} 个账号已领完`];
  if (view.remaining.length) pieces.push(`未完成：${view.remaining.join("、")}`);
  else if (view.claimable > 0) pieces.push(`还剩 ${view.claimable} 项可领`);
  if (view.error > 0) pieces.push(`${view.error} 个账号失败，可重试`);
  if (view.unconfirmed > 0) pieces.push(`${view.unconfirmed} 个结果待确认`);
  return `${pieces.join("；")}${when}`;
}

/** 卡片 chip 文案：`已领完` / `3 项待领` / `待确认` / `无任务`。 */
export function claimChipLabel(entry: ClaimLedgerEntry | undefined): string | null {
  if (!entry) return null;
  switch (entry.state) {
    case "claimed":
      return "已领完";
    case "blocked":
      return "无任务";
    case "skipped":
      return null;
    case "unconfirmed":
      return "待确认";
    case "error":
      return "领取失败";
    default:
      return entry.counts.claimable > 0 ? `${entry.counts.claimable} 项待领` : "未领完";
  }
}

/** 卡片 chip tooltip：三档计数 + 剩余任务名。 */
export function claimChipHint(entry: ClaimLedgerEntry | undefined): string {
  if (!entry) return "";
  const counts = entry.counts;
  const parts: string[] = [];
  if (counts.total > 0) {
    parts.push(`已完成 ${counts.claimed} · 可领 ${counts.claimable} · 未达标 ${counts.blocked}`);
  }
  if (entry.message) parts.push(entry.message);
  if (entry.tasks.length) {
    const labels = entry.tasks.slice(0, 3).map((task) => task.label).join("、");
    const more = entry.tasks.length > 3 ? ` 等 ${entry.tasks.length} 项` : "";
    parts.push(`未完成：${labels}${more}`);
  }
  if (entry.updatedAt) parts.push(`${formatClock(entry.updatedAt)} 核对`);
  return parts.join("；");
}

function formatClock(ts: number): string {
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "";
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}
