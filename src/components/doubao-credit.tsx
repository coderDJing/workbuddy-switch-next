import type { DoubaoCredit } from "@/lib/api";

export const doubaoTime = (at?:number|null) => at ? new Date(at*1000).toLocaleString("zh-CN",{hour12:false,month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit"}) : "—";
export const percent = (v?:number|null) => v == null ? "—" : `${new Intl.NumberFormat("zh-CN",{maximumFractionDigits:1}).format(v)}%`;
export function DoubaoCreditBlock({credit}:{credit:DoubaoCredit|null}) {
  if(!credit)return <p className="py-3 text-xs leading-6 text-muted-foreground">尚未查询额度，点击「刷新额度」获取官方状态。</p>;
  return <div className="space-y-3 text-xs text-muted-foreground">
    <div className="flex justify-between gap-2"><span>{credit.planActive ? credit.planName ?? "订阅会员" : "未开通个人订阅"}</span>{credit.planExpiresAt && <span>到期 {doubaoTime(credit.planExpiresAt)}</span>}</div>
    {credit.windows.length ? credit.windows.map(w=><div key={w.key} className="space-y-1.5"><div className="flex justify-between gap-2"><span>{w.name}</span><span className="font-medium tabular-nums text-foreground">已用 {percent(w.usedPercent)}</span></div><div role="progressbar" aria-label={`${w.name}已用 ${percent(w.usedPercent)}`} aria-valuenow={w.usedPercent ?? undefined} aria-valuemin={0} aria-valuemax={100} className="h-2 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-primary" style={{width:percent(w.usedPercent ?? 0)}} /></div><div className="flex justify-between gap-2 text-[11px]"><span>{w.exhausted ? "额度已用尽" : w.remainingPercent == null ? "额度未知" : `剩余 ${percent(w.remainingPercent)}`}</span><span>{w.resetsAt ? `${doubaoTime(w.resetsAt)} 重置` : "重置时间未知"}</span></div></div>) : <p className="rounded-lg bg-muted/40 p-3 leading-5">官方未返回可用的额度窗口。可查看下方签到活动；签到奖励与订阅额度分别计算。</p>}
    <p className="text-[11px]">更新于 {doubaoTime(credit.checkedAt)}</p>
  </div>;
}
