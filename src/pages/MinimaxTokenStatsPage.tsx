import { useEffect, useMemo, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import * as api from "@/lib/api";

const number = (n?: number | null) => n == null ? "—" : new Intl.NumberFormat("zh-CN").format(n);
const config = { total: { label: "Token", color: "var(--data-series-violet)" } };
const empty = (): api.MinimaxTokens => ({ input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0, requests: 0 });
export default function MinimaxTokenStatsPage() {
  const [stats, setStats] = useState<api.MinimaxTokenStatistics | null>(null);
  const [loading, setLoading] = useState(false), [error, setError] = useState("");
  const [reload, setReload] = useState(0), [range, setRange] = useState("30"), [model, setModel] = useState("all");
  useEffect(() => {
    let cancelled = false; setLoading(true); setError("");
    (api.minimaxAction("token_stats", {}) as Promise<api.MinimaxTokenStatistics>).then(value => { if (!cancelled) setStats(value); }).catch(error => { if (!cancelled) setError(api.asError(error)); }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [reload]);
  const view = useMemo(() => {
    const now = (stats?.generatedAt ?? Date.now() / 1000) * 1000;
    const start = range === "all" ? "" : new Intl.DateTimeFormat("sv-SE").format(now - (Number(range) - 1) * 86400000);
    const total = empty(), days = new Map<string, number>(), models = new Map<string, api.MinimaxTokens>();
    for (const row of stats?.daily ?? []) {
      if (row.date < start || (model !== "all" && row.model !== model)) continue;
      const m = models.get(row.model) ?? empty();
      for (const key of Object.keys(total) as (keyof api.MinimaxTokens)[]) { total[key] += row.tokens[key]; m[key] += row.tokens[key]; }
      models.set(row.model, m); days.set(row.date, (days.get(row.date) ?? 0) + row.tokens.total);
    }
    return { total, daily: [...days].map(([date, total]) => ({ date, total })), models: [...models].sort((a, b) => b[1].total - a[1].total) };
  }, [stats, range, model]);
  return <div className="mx-auto w-full max-w-[1180px] space-y-6 px-4 py-6 sm:px-8 sm:py-9">
    <header className="flex flex-wrap items-start justify-between gap-4"><div><h1 className="text-[28px] font-semibold tracking-tight">Token 统计</h1><p className="mt-2 text-sm text-muted-foreground">MiniMax Code 本机会话用量，桌面端与 CLI 合并统计。</p></div><Button variant="outline" disabled={loading} onClick={() => setReload(v => v + 1)}>{loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}刷新统计</Button></header>
    <p className="rounded-xl border bg-muted/30 p-4 text-sm leading-6 text-muted-foreground">本地会话没有可靠的账号与区域标识，此页汇总本机所有账号、国内版和国际版的记录。缓存读取和缓存写入分别统计，不换算为积分。</p>
    <div className="flex gap-3"><Select value={range} onValueChange={setRange}><SelectTrigger className="w-36" aria-label="统计时间范围"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="7">近 7 天</SelectItem><SelectItem value="30">近 30 天</SelectItem><SelectItem value="all">全部历史</SelectItem></SelectContent></Select><Select value={model} onValueChange={setModel}><SelectTrigger className="w-52" aria-label="统计模型"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">全部模型</SelectItem>{stats?.models.map(m => <SelectItem key={m.model} value={m.model}>{m.model}</SelectItem>)}</SelectContent></Select></div>
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    {stats && !stats.complete && <p className="text-sm text-amber-700">有 {stats.skippedRecords} 处记录不完整或超出读取上限，当前统计为已读取部分。</p>}
    <Card className="py-0"><CardContent className="grid gap-5 p-5 sm:grid-cols-3 lg:grid-cols-6">{([["总 Token", "total"], ["新输入", "input"], ["输出", "output"], ["缓存读取", "cacheRead"], ["缓存写入", "cacheWrite"], ["请求", "requests"]] as const).map(([label, key]) => <div key={key}><p className="text-xs text-muted-foreground">{label}</p><p className="mt-3 text-xl font-semibold tabular-nums">{number(stats?.available ? view.total[key] : null)}</p></div>)}</CardContent></Card>
    <Card><CardHeader><CardTitle>每日用量</CardTitle><CardDescription>按本机时区汇总已记录的 Token。</CardDescription></CardHeader><CardContent>{view.daily.length ? <ChartContainer config={config} className="h-64 w-full"><BarChart data={view.daily}><CartesianGrid vertical={false} /><XAxis dataKey="date" /><YAxis /><ChartTooltip content={<ChartTooltipContent />} /><Bar dataKey="total" fill="var(--color-total)" radius={[4, 4, 0, 0]} /></BarChart></ChartContainer> : <p className="py-12 text-center text-sm text-muted-foreground">{loading ? "正在读取用量…" : "当前范围没有可用的 Token 记录。"}</p>}</CardContent></Card>
    <Card><CardHeader><CardTitle>模型用量</CardTitle></CardHeader><CardContent><Table><TableHeader><TableRow><TableHead>模型</TableHead><TableHead>Token</TableHead><TableHead>输入</TableHead><TableHead>输出</TableHead><TableHead>缓存读取 / 写入</TableHead><TableHead>请求</TableHead></TableRow></TableHeader><TableBody>{view.models.map(([name, v]) => <TableRow key={name}><TableCell>{name}</TableCell><TableCell>{number(v.total)}</TableCell><TableCell>{number(v.input)}</TableCell><TableCell>{number(v.output)}</TableCell><TableCell>{number(v.cacheRead)} / {number(v.cacheWrite)}</TableCell><TableCell>{number(v.requests)}</TableCell></TableRow>)}</TableBody></Table></CardContent></Card>
  </div>;
}
