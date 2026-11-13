import { Clock3, Gift, Info, ListChecks, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { ClaimLedgerEntry, ClaimState } from "@/lib/api";
import { claimStateLabel, claimTaskLabel } from "@/lib/claim-status";
import { cn } from "@/lib/utils";

interface ClaimLedgerDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  /** 该平台 / 档位下今天的领取记忆。 */
  entries: ClaimLedgerEntry[];
  /** 账号 ID → 展示名；缺失时回落到 ID 前缀。 */
  accountNames?: Record<string, string>;
  /** 今天还没有任何记录的账号，一并列出，避免「没显示＝没问题」的误解。 */
  pendingAccounts?: { id: string; name: string }[];
  /** 弹窗里的主操作：打开面板看一眼，再决定是否真的去领。 */
  action?: {
    label: string;
    /** 按钮左侧的说明，例如「会为 8 个账号跑一遍，可能需要几分钟」。 */
    note?: string;
    running?: boolean;
    onClick: () => void;
  };
}

/** 账号级状态色调：与卡片 chip 保持一致。 */
function stateTone(state: ClaimState | undefined): string {
  switch (state) {
    case "claimed":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "error":
    case "unconfirmed":
      return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "partial":
      return "border-primary/30 bg-primary/10 text-primary";
    default:
      return "border-border bg-muted/40 text-muted-foreground";
  }
}

/** 任务级状态色调。 */
function taskTone(state: string): string {
  switch (state) {
    case "claimed":
      return "text-emerald-700 dark:text-emerald-300";
    case "partial":
      return "text-primary";
    case "blocked":
      return "text-muted-foreground";
    default:
      return "text-amber-700 dark:text-amber-300";
  }
}

function clockOf(ts: number): string {
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return "";
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

const SOURCE_LABEL: Record<string, string> = {
  manual: "手动领取",
  auto: "后台自动",
  server: "官方状态",
};

/**
 * 今日领取详情：把「已完成 / 可领 / 未达标」和具体任务名摊开。
 *
 * tooltip 只能短暂看一眼，也不能复制；这个弹窗是它的完整版——逐账号列出
 * 未完成任务、原因、核对时间和触发来源。
 */
export function ClaimLedgerDialog({
  open,
  onOpenChange,
  title,
  description,
  entries,
  accountNames,
  pendingAccounts = [],
  action,
}: ClaimLedgerDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {description ??
              "记录按北京时间每天 0 点换新。可领＝现在还能领；未达标＝条件未满足或需要在官方客户端操作；待确认＝结果未知，不会重复提交。"}
          </DialogDescription>
        </DialogHeader>
        <ClaimLedgerList entries={entries} accountNames={accountNames} pendingAccounts={pendingAccounts} />
        <DialogFooter className="items-center gap-2 sm:justify-between">
          <span className="flex min-w-0 items-center gap-1.5 text-[11px] leading-4 text-muted-foreground">
            <Info className="size-3.5 shrink-0" />
            <span className="min-w-0 break-words">
              {action?.note ?? "需要重新核对时，可在设置页清除今日记录"}
            </span>
          </span>
          <div className="flex shrink-0 items-center gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              关闭
            </Button>
            {action && (
              <Button onClick={action.onClick} disabled={action.running}>
                {action.running ? <Loader2 className="animate-spin" /> : <Gift />}
                {action.running ? "领取中…" : action.label}
              </Button>
            )}
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** 弹窗正文：与外壳分离，方便静态渲染测试。 */
export function ClaimLedgerList({
  entries,
  accountNames,
  pendingAccounts = [],
}: Omit<ClaimLedgerDialogProps, "open" | "onOpenChange" | "title" | "description">) {
  const nameOf = (id: string) => accountNames?.[id] ?? id.slice(0, 8);
  const empty = entries.length === 0 && pendingAccounts.length === 0;

  return (
    <>
      {empty ? (
          <div className="rounded-xl border border-dashed border-border bg-muted/15 px-4 py-10 text-center text-sm text-muted-foreground">
            <p>今天还没有领取记录。</p>
            <p className="mt-2 text-xs">点一次「领取」后，这里会按账号列出具体任务。</p>
          </div>
        ) : (
          <div className="-mx-1 max-h-[60vh] space-y-2 overflow-y-auto px-1">
            {entries.map((entry) => {
              const remaining = entry.tasks ?? [];
              return (
                <section key={`${entry.accountId}-${entry.day}`} className="rounded-xl border border-border">
                  <header className="flex items-center justify-between gap-3 border-b border-border px-3 py-2">
                    <span className="min-w-0 flex-1 truncate text-sm font-medium" title={nameOf(entry.accountId)}>
                      {nameOf(entry.accountId)}
                    </span>
                    <span
                      className={cn(
                        "shrink-0 rounded-md border px-1.5 py-0.5 text-[11px] font-medium leading-4",
                        stateTone(entry.state),
                      )}
                    >
                      {claimStateLabel(entry.state)}
                    </span>
                  </header>

                  <div className="space-y-2.5 px-3 py-2.5">
                    <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                      <span className="inline-flex items-center gap-1">
                        <ListChecks className="size-3.5" />
                        已完成 <b className="font-semibold text-foreground">{entry.counts.claimed}</b>
                      </span>
                      <span>
                        可领 <b className="font-semibold text-foreground">{entry.counts.claimable}</b>
                      </span>
                      <span>
                        未达标 <b className="font-semibold text-foreground">{entry.counts.blocked}</b>
                      </span>
                      <span>共 {entry.counts.total} 项</span>
                    </p>

                    {remaining.length > 0 ? (
                      <ul className="space-y-1">
                        {remaining.map((task, index) => (
                          <li
                            key={`${task.label}-${index}`}
                            className="flex items-baseline justify-between gap-3 text-xs"
                          >
                            <span className="min-w-0 flex-1 break-words text-foreground">{task.label}</span>
                            <span className={cn("shrink-0 text-[11px]", taskTone(task.state))}>
                              {claimTaskLabel(task.state)}
                            </span>
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="text-xs text-muted-foreground">没有未完成的任务。</p>
                    )}

                    {entry.message && (
                      <p className="rounded-lg bg-muted/40 px-2 py-1.5 text-[11px] leading-4 text-muted-foreground">
                        {entry.message}
                      </p>
                    )}

                    <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
                      <Clock3 className="size-3" />
                      {clockOf(entry.updatedAt)} 核对
                      <span aria-hidden="true">·</span>
                      {SOURCE_LABEL[entry.source] ?? entry.source}
                      {entry.attemptCount > 1 && (
                        <>
                          <span aria-hidden="true">·</span>
                          今天第 {entry.attemptCount} 次
                        </>
                      )}
                    </p>
                  </div>
                </section>
              );
            })}
            {pendingAccounts.map((account) => (
              <section key={`pending-${account.id}`} className="rounded-xl border border-dashed border-border">
                <header className="flex items-center justify-between gap-3 px-3 py-2">
                  <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground" title={account.name}>
                    {account.name}
                  </span>
                  <span className="shrink-0 rounded-md border border-border bg-muted/40 px-1.5 py-0.5 text-[11px] font-medium leading-4 text-muted-foreground">
                    今天还没跑过
                  </span>
                </header>
              </section>
            ))}
          </div>
      )}
    </>
  );
}
