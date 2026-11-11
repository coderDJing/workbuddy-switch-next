import { useEffect, useMemo, useState } from "react";
import { ListTree, Loader2, RefreshCw } from "lucide-react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { MinimaxMark } from "@/components/product-marks";
import { DemoAction } from "@/components/demo-action";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { observedMinimaxUsage } from "@/lib/minimax-statistics";
const normalizeMinimaxRegion = (value:string):api.MinimaxRegion => value === "en" ? "en" : "cn";
import { useMinimaxView } from "@/stores/minimax-view";

const number = (value?: number | null) => value == null ? "—" : new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 2 }).format(value);
const time = (at?: number) => at ? new Date(at * 1000).toLocaleString("zh-CN", { hour12: false }) : "—";
const chartConfig = { consumed: { label: "观测新增用量", color: "var(--data-series-violet)" } };

export default function MinimaxCreditStatsPage() {
  const { region, setRegion } = useMinimaxView();
  const [stats, setStats] = useState<api.MinimaxStatistics | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [range, setRange] = useState("30");
  const [accountId, setAccountId] = useState("all");
  const unit = "credits";
  const [detail, setDetail] = useState(false);
  const [page, setPage] = useState(0);
  const [reload, setReload] = useState(0);
  const [refreshRemote, setRefreshRemote] = useState(false);
  useEffect(() => {
    let cancelled = false; setLoading(true); setError("");
    api.getMinimaxStatistics(region, refreshRemote).then(value => { if (!cancelled) setStats(value); }).catch(error => { if (!cancelled) setError(api.asError(error)); }).finally(() => { if (!cancelled) { setLoading(false); setRefreshRemote(false); } });
    return () => { cancelled = true; };
  }, [region, reload]);
  const view = stats?.region === region ? stats : null;
  const accountOptions = useMemo(() => [...new Map([...(view?.accounts ?? []).map(a => [a.id, a.name] as const), ...(view?.snapshots ?? []).map(s => [s.accountId, s.name] as const)]).entries()], [view]);
  const snapshots = (view?.snapshots ?? []).filter(s => accountId === "all" || s.accountId === accountId);
  const end = view?.generatedAt ?? Math.floor(Date.now() / 1000);
  const start = range === "all" ? 0 : end - Number(range) * 86400;
  const usage = observedMinimaxUsage(snapshots, start, end);
  const rows = snapshots.filter(s => s.at >= start && s.at <= end && s.credit.unit === unit).sort((a, b) => b.at - a.at);
  const accounts = (view?.accounts ?? []).filter(a => accountId === "all" || a.id === accountId);
  const balances = accounts.map(a => a.credits).filter((c): c is api.MinimaxCredit => !!c && c.unit === unit && c.available && !c.unlimited && c.remaining != null);
  const remaining = balances.length ? balances.reduce((sum, c) => sum + c.remaining!, 0) : null;
  const expiring = balances.filter(c => c.expiresAt != null && c.expiresAt > end && c.expiresAt - end <= 7 * 86400).length;
  const suffix = "积分";
  return <div className="mx-auto w-full max-w-[1180px] space-y-6 px-4 py-6 sm:px-8 sm:py-9">
    <header className="flex flex-wrap items-start justify-between gap-4"><div><h1 className="text-[28px] font-semibold tracking-tight">积分统计</h1><p className="mt-2 text-sm leading-6 text-muted-foreground">MiniMax Code 官方额度快照与观测用量 · 更新于 {time(view?.generatedAt)}</p></div><div className="flex flex-wrap gap-2"><Button variant="outline" size="sm" disabled={!rows.length} onClick={() => { setPage(0); setDetail(true); }}><ListTree />查看明细</Button><DemoAction><Button variant="outline" size="sm" disabled={loading} onClick={() => { setRefreshRemote(true); setReload(v => v + 1); }}>{loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}刷新统计</Button></DemoAction></div></header>
    <div className="flex flex-wrap items-center gap-3"><Tabs value={region} onValueChange={value => { setRefreshRemote(false); setRegion(normalizeMinimaxRegion(value)); setAccountId("all"); }}><TabsList aria-label="MiniMax Code 统计区域"><TabsTrigger value="cn"><MinimaxMark size={16} />国内版</TabsTrigger><TabsTrigger value="en"><MinimaxMark size={16} global />国际版</TabsTrigger></TabsList></Tabs><Select value={accountId} onValueChange={setAccountId}><SelectTrigger className="w-44" aria-label="统计账号"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">全部账号</SelectItem>{accountOptions.map(([id, name]) => <SelectItem key={id} value={id}>{name}</SelectItem>)}</SelectContent></Select><Select value={range} onValueChange={setRange}><SelectTrigger className="w-32" aria-label="统计时间范围"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="7">近 7 天</SelectItem><SelectItem value="30">近 30 天</SelectItem><SelectItem value="all">全部历史</SelectItem></SelectContent></Select></div>
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    {!!view?.errors.length && <div className="rounded-xl border border-destructive/20 p-4 text-sm text-destructive">{view.errors.map(e => <p key={e.accountId}>{view.accounts.find(a => a.id === e.accountId)?.name ?? "账号"}：{e.message}</p>)}</div>}
    <Card className="gap-0 overflow-hidden rounded-2xl bg-card/70 py-0 shadow-none"><CardContent className="grid gap-0 p-0 sm:grid-cols-4">{[["已保存账号", String(accounts.length)], [`已知剩余${suffix}`, number(remaining)], [`观测消耗（${suffix}）`, number(usage.total)], ["7 天内到期账号", remaining == null ? "—" : String(expiring)]].map(([label, value]) => <div key={label} className="border-b p-5 sm:border-b-0 sm:border-r last:border-0"><p className="text-xs text-muted-foreground">{label}</p><p className="mt-3 text-2xl font-semibold tabular-nums">{value}</p></div>)}</CardContent></Card>
    <Card className="rounded-2xl shadow-none"><CardHeader><CardTitle>观测用量趋势</CardTitle><CardDescription>根据积分包组成不变时相邻快照的已用积分差值记录。领取新积分、积分包到期或查询失败时重新建立基线，不推算缺失用量。</CardDescription></CardHeader><CardContent>{usage.daily.length ? <ChartContainer config={chartConfig} className="h-64 w-full"><BarChart data={usage.daily}><CartesianGrid vertical={false} /><XAxis dataKey="date" tickLine={false} axisLine={false} /><YAxis tickLine={false} axisLine={false} /><ChartTooltip content={<ChartTooltipContent />} /><Bar dataKey="consumed" fill="var(--color-consumed)" radius={[4, 4, 0, 0]} /></BarChart></ChartContainer> : <div className="rounded-xl border border-dashed px-4 py-12 text-center text-sm text-muted-foreground">{loading ? "正在读取统计…" : "需要至少两次处于同一额度周期的有效查询，才会记录观测消耗。"}</div>}</CardContent></Card>
    <Card className="rounded-2xl shadow-none"><CardHeader><CardTitle>账号额度</CardTitle><CardDescription>余额来自最近一次官方查询；无限额度与未知额度分别保留。历史快照最多保留 {view?.retentionDays ?? 90} 天。</CardDescription></CardHeader><CardContent><div className="overflow-x-auto"><Table><TableHeader><TableRow><TableHead>账号</TableHead><TableHead>剩余额度</TableHead><TableHead>已用 / 总额</TableHead><TableHead>查询时间</TableHead></TableRow></TableHeader><TableBody>{accounts.map(a => <TableRow key={a.id}><TableCell>{a.name}</TableCell><TableCell>{a.credits?.unlimited ? "无限制" : a.credits?.available ? number(a.credits.remaining) : "—"} {a.credits?.unit === "usd" ? "美元" : a.credits?.unit === "credits" ? "积分" : ""}</TableCell><TableCell>{number(a.credits?.used)} / {number(a.credits?.total)}</TableCell><TableCell className="whitespace-nowrap text-muted-foreground">{time(a.credits?.checkedAt)}</TableCell></TableRow>)}</TableBody></Table></div>{!accounts.length && <p className="py-8 text-center text-sm text-muted-foreground">添加账号并查询额度后，即可开始记录统计。</p>}</CardContent></Card>
    <Dialog open={detail} onOpenChange={setDetail}><DialogContent className="flex max-h-[85vh] flex-col sm:max-w-4xl"><DialogHeader><DialogTitle>MiniMax Code 额度快照明细</DialogTitle><DialogDescription>展示官方查询结果，不包含令牌或对话内容。</DialogDescription></DialogHeader><div className="min-h-0 overflow-auto"><Table><TableHeader><TableRow><TableHead>时间</TableHead><TableHead>账号</TableHead><TableHead>剩余</TableHead><TableHead>已用 / 总额</TableHead></TableRow></TableHeader><TableBody>{rows.slice(page * 50, page * 50 + 50).map((s, i) => <TableRow key={`${s.accountId}:${s.at}:${i}`}><TableCell className="whitespace-nowrap">{time(s.at)}</TableCell><TableCell>{s.name}</TableCell><TableCell>{s.credit.unlimited ? "无限制" : number(s.credit.remaining)}</TableCell><TableCell>{number(s.credit.used)} / {number(s.credit.total)}</TableCell></TableRow>)}</TableBody></Table></div><div className="flex justify-end gap-3"><Button variant="outline" size="sm" disabled={page === 0} onClick={() => setPage(p => p - 1)}>上一页</Button><span className="self-center text-xs text-muted-foreground">{page + 1} / {Math.max(1, Math.ceil(rows.length / 50))}</span><Button variant="outline" size="sm" disabled={(page + 1) * 50 >= rows.length} onClick={() => setPage(p => p + 1)}>下一页</Button></div></DialogContent></Dialog>
  </div>;
}
