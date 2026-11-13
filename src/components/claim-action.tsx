import type { ReactNode } from "react";
import { BadgeCheck, Ban, Gift, ListChecks, Loader2 } from "lucide-react";

import { DemoAction } from "@/components/demo-action";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { claimButtonHint, claimButtonLabel, claimChipHint, claimChipLabel, type ClaimView } from "@/lib/claim-status";
import type { ClaimLedgerEntry } from "@/lib/api";
import { cn } from "@/lib/utils";

interface ClaimActionButtonProps {
  view: ClaimView;
  /** 领取动作进行中（自身 loading 态）。 */
  running: boolean;
  /** 其他原因禁用（无账号、其他动作进行中）。 */
  disabled?: boolean;
  /** 未领完时的文案，如「领取积分」「领取奖励」。 */
  idleLabel?: string;
  /** 没有记忆时的默认 tooltip。 */
  hint: string;
  /** 点击后打开今日领取详情；真正的执行放在弹窗里。 */
  onOpen: () => void;
  /** 未领完时的图标，默认礼盒。 */
  icon?: ReactNode;
  className?: string;
}

/**
 * 统一领取按钮：只负责展示今天的状态并打开详情弹窗。
 *
 * 点一次不再直接跑几分钟的任务——先让用户看见「还剩哪几项、哪些账号没跑」，
 * 再在弹窗里点「领取全部 / 重新核对」。`running` 时按钮仅展示进度，不再可点。
 */
export function ClaimActionButton({
  view,
  running,
  disabled = false,
  idleLabel = "领取积分",
  hint,
  onOpen,
  icon,
  className,
}: ClaimActionButtonProps) {
  const settled = view.mode === "done" || view.mode === "blocked";
  const label = running ? "领取中…" : settled ? claimButtonLabel(view) : idleLabel;
  const tooltip = running ? "正在执行领取任务…" : claimButtonHint(view, hint);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span>
          <DemoAction>
            <Button
              variant={settled ? "ghost" : "outline"}
              size="sm"
              className={cn("h-9 rounded-lg text-xs", className)}
              disabled={running || disabled}
              onClick={() => {
                if (running || disabled) return;
                onOpen();
              }}
              aria-label={running ? label : `${label}：打开今日领取详情`}
            >
              {running ? (
                <Loader2 className="animate-spin" />
              ) : view.mode === "done" ? (
                <BadgeCheck />
              ) : view.mode === "blocked" ? (
                <Ban />
              ) : (
                icon ?? <Gift />
              )}
              {label}
              {view.mode === "partial" && view.claimable > 0 ? (
                <span
                  className="ml-0.5 rounded-full bg-primary/10 px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-primary"
                  aria-hidden="true"
                >
                  剩 {view.claimable}
                </span>
              ) : null}
            </Button>
          </DemoAction>
        </span>
      </TooltipTrigger>
      <TooltipContent side="top">{tooltip}</TooltipContent>
    </Tooltip>
  );
}

/**
 * 平台卡片上的领取状态标记：`已领完` / `3 项待领` / `待确认` / `无任务`。
 *
 * 与按钮共用同一份派生逻辑，只是把结果放在账号维度展示——用户不用点按钮
 * 就知道哪个账号今天还有东西可领。
 */
export function ClaimStateBadge({
  entry,
  className,
  onClick,
}: {
  entry?: ClaimLedgerEntry;
  className?: string;
  /** 提供时渲染为按钮，打开今日领取详情。 */
  onClick?: () => void;
}) {
  const label = claimChipLabel(entry);
  if (!entry || !label) return null;
  const tone =
    entry.state === "claimed"
      ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
      : entry.state === "error" || entry.state === "unconfirmed"
        ? "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
        : "border-border bg-muted/40 text-muted-foreground";
  const classes = cn(
    "inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium leading-4",
    tone,
    className,
  );
  const content = (
    <>
      <ListChecks className="size-3" />
      {label}
    </>
  );
  if (onClick) {
    return (
      <button
        type="button"
        onClick={onClick}
        title={claimChipHint(entry)}
        aria-label={`查看今日领取详情（${label}）`}
        className={cn(classes, "cursor-pointer transition-opacity hover:opacity-80")}
      >
        {content}
      </button>
    );
  }
  return (
    <span title={claimChipHint(entry)} className={classes}>
      {content}
    </span>
  );
}
