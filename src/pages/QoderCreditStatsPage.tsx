import { useEffect, useMemo, useState } from "react";
import { CircleAlert, ListTree, Loader2, RefreshCw } from "lucide-react";
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { ChartContainer, ChartTooltip, ChartTooltipContent } from "@/components/ui/chart";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { QoderMark } from "@/components/product-marks";
import { DemoAction } from "@/components/demo-action";
import * as api from "@/lib/api";
import type { QoderCreditStatistics, QoderStatistics } from "@/lib/types";
import { localDateKey, qoderCreditView, qoderRangeStart, type QoderStatsRange } from "@/lib/qoder-statistics";
import { useQoderView } from "@/stores/qoder-view";

const format = (value: number) => new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 3 }).format(value);
const time = (value: number) => new Date(value).toLocaleString("zh-CN", { hour12: false });
const chartConfig = {
  billableCredits: { label: "计费请求积分", color: "var(--data-series-violet)" },
  nonBillableCredits: { label: "非计费请求积分", color: "var(--data-series-teal)" },
  unknownBillingCredits: { label: "计费状态未知", color: "var(--data-series-amber)" },
  credits: { label: "观察到的消耗", color: "var(--data-series-violet)" },
};

function Empty({ children }: { children: React.ReactNode }) {
  return <div className="rounded-xl border border-dashed px-4 py-12 text-center text-sm leading-6 text-muted-foreground">{children}</div>;
}

function Metrics({ values }: { values: [string, number | null][] }) {
  return <Card className="gap-0 overflow-hidden rounded-2xl bg-card/70 py-0 shadow-none"><CardContent className="grid grid-cols-2 divide-border/60 p-0 lg:grid-cols-4">
    {values.map(([label, value]) => <div key={label} className="min-w-0 border-r border-b border-border/60 px-4 py-6 text-center last:border-r-0 lg:border-b-0">
      <p className="text-xs text-muted-foreground">{label}</p><p className="mt-3 break-all text-2xl font-semibold tabular-nums">{value === null ? "—" : format(value)}</p>
    </div>)}
  </CardContent></Card>;
}

export default function QoderCreditStatsPage() {
  const variant = useQoderView((s) => s.variant), setVariant = useQoderView((s) => s.setVariant);
  const [local, setLocal] = useState<QoderStatistics | null>(null);
  const [quota, setQuota] = useState<QoderCreditStatistics | null>(null);
  const [loading, setLoading] = useState(true), [refreshing, setRefreshing] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null), [quotaError, setQuotaError] = useState<string | null>(null);
  const [reload, setReload] = useState(0), [quotaReload, setQuotaReload] = useState(0);
  const [range, setRange] = useState<QoderStatsRange>("30d");
  const [model, setModel] = useState("all"), [account, setAccount] = useState("all");
  const [detail, setDetail] = useState(false), [page, setPage] = useState(0);
  const [tab, setTab] = useState("requests");
  useEffect(() => {
    let disposed = false; setLoading(true); setLocalError(null);
    api.getQoderStatistics().then((data) => { if (!disposed) setLocal(data); })
      .catch((e) => { if (!disposed) setLocalError(api.asError(e)); }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [reload]);
  useEffect(() => {
    let disposed = false; setRefreshing(true); setQuotaError(null);
    // Entering the page reads snapshots; explicit refresh queries saved accounts.
    api.getQoderCreditStatistics(quotaReload > 0).then((data) => { if (!disposed) setQuota(data); })
      .catch((e) => { if (!disposed) setQuotaError(api.asError(e)); }).finally(() => { if (!disposed) setRefreshing(false); });
    return () => { disposed = true; };
  }, [quotaReload]);
  useEffect(() => { setModel("all"); setAccount("all"); setPage(0); }, [variant]);
  useEffect(() => { setPage(0); }, [range, model, local]);
  const source = (local?.credits ?? []).find((s) => s.source === (variant === "ai" ? "qoder-ai" : "qoder"));
  const view = useMemo(() => source ? qoderCreditView(source, range, model) : null, [source, range, model]);
  const allAccounts = (quota?.accounts ?? []).filter((a) => a.global === (variant === "ai"));
  const accounts = allAccounts.filter((a) => account === "all" || a.accountKey === account);
  const start = qoderRangeStart(range), end = localDateKey(new Date());
  const accountDaily = useMemo(() => {
    const days = new Map<string, number>();
    (quota?.daily ?? []).filter((d) => d.global === (variant === "ai") && (account === "all" || d.accountKey === account) && d.date >= start && d.date <= end)
      .forEach((d) => days.set(d.date, (days.get(d.date) ?? 0) + d.credits));
    return [...days].sort(([a], [b]) => a.localeCompare(b)).map(([key, credits]) => ({ key, credits }));
  }, [quota, variant, account, start, end]);
  const chart = (data: { key: string }[], fields: (keyof typeof chartConfig)[]) => data.length === 0 ? <Empty>所选范围暂无可用记录。</Empty> :
    <ChartContainer config={chartConfig} className="h-72 w-full"><BarChart data={data} accessibilityLayer>
      <CartesianGrid vertical={false} /><XAxis dataKey="key" tickFormatter={(s: string) => s.slice(5)} tickLine={false} axisLine={false} minTickGap={24} />
      <YAxis tickLine={false} axisLine={false} width={60} /><ChartTooltip content={<ChartTooltipContent />} />
      {fields.map((field) => <Bar key={field} dataKey={field} stackId="credits" fill={`var(--color-${field})`} maxBarSize={36} />)}
    </BarChart></ChartContainer>;
  return <div className="mx-auto w-full max-w-[1180px] min-w-0 px-4 py-6 sm:px-8 sm:py-9">
    <header className="mb-6 flex flex-wrap items-start justify-between gap-4"><div>
      <p className="mb-2 text-[11px] font-semibold tracking-[0.16em] text-primary">QODER / INSIGHTS</p>
      <h1 className="text-[28px] font-semibold tracking-tight">积分统计</h1>
      <p className="mt-2 text-sm text-muted-foreground">Qoder · 本地用量更新于 {local ? time(local.generatedAt) : "—"}</p>
    </div><div className="flex flex-wrap gap-2">
      {tab === "requests" && <Button variant="outline" size="sm" onClick={() => setDetail(true)} disabled={!view?.requests.length}><ListTree />请求明细</Button>}
      <DemoAction><Button variant="outline" size="sm" disabled={loading || refreshing} onClick={() => { setReload((n) => n + 1); setQuotaReload((n) => n + 1); }}>
        {loading || refreshing ? <Loader2 className="animate-spin" /> : <RefreshCw />}刷新统计
      </Button></DemoAction>
    </div></header>
    <Tabs value={variant} onValueChange={(v) => setVariant(v === "ai" ? "ai" : "cn")} className="mb-6">
      <TabsList aria-label="Qoder 档位"><TabsTrigger value="cn"><QoderMark size={16} />国内版</TabsTrigger><TabsTrigger value="ai"><QoderMark size={16} global />国际版</TabsTrigger></TabsList>
    </Tabs>
    {(localError || quotaError) && <Alert variant="destructive" className="mb-6"><CircleAlert /><AlertTitle>部分统计加载失败</AlertTitle><AlertDescription>{localError || quotaError}，可点击刷新重试。已加载数据仍保留。</AlertDescription></Alert>}
    <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
      <Tabs value={tab} onValueChange={setTab}><TabsList><TabsTrigger value="requests">本地请求</TabsTrigger><TabsTrigger value="accounts">账号额度</TabsTrigger></TabsList></Tabs>
      <Tabs value={range} onValueChange={(r) => setRange(r as QoderStatsRange)}><TabsList aria-label="统计时间范围">
        <TabsTrigger value="today">今日</TabsTrigger><TabsTrigger value="7d">近 7 天</TabsTrigger><TabsTrigger value="30d">近 30 天</TabsTrigger><TabsTrigger value="all">全部</TabsTrigger>
      </TabsList></Tabs>
    </div>
    {tab === "requests" ? <div className="space-y-8">
      <p className="text-sm leading-6 text-muted-foreground">按本机桌面端与 CLI 共用的会话记录去重统计。请求积分包含计费与非计费部分，不等同于账号扣款；历史记录缺少账号标识，不归入当前账号。其他设备及已删除记录不在覆盖范围内。</p>
      {loading && !local ? <Skeleton className="h-48 w-full" /> : view && source ? <>
        <Select value={model} onValueChange={setModel}><SelectTrigger className="w-full sm:w-64" aria-label="筛选模型"><SelectValue /></SelectTrigger><SelectContent>
          <SelectItem value="all">全部模型</SelectItem>{source.models.map((m) => <SelectItem key={m.key} value={m.key}>{m.key}</SelectItem>)}
        </SelectContent></Select>
        <Metrics values={[["请求积分", view.summary.records ? view.summary.credits : null], ["计费请求积分", view.summary.records ? view.summary.billableCredits : null], ["非计费请求积分", view.summary.records ? view.summary.nonBillableCredits : null], ["请求次数", view.summary.records]]} />
        {view.summary.unknownBillingCredits > 0 && <p className="text-sm text-muted-foreground">另有 {format(view.summary.unknownBillingCredits)} 积分未提供计费状态。</p>}
        <Card className="shadow-none"><CardHeader><CardTitle>每日请求积分</CardTitle><CardDescription>紫色为计费，青色为非计费，琥珀色为计费状态未知。</CardDescription></CardHeader><CardContent>{chart(view.daily, ["billableCredits", "nonBillableCredits", "unknownBillingCredits"])}</CardContent></Card>
        <Card className="shadow-none"><CardHeader><CardTitle>模型用量排行</CardTitle><CardDescription>与所选日期范围一致，按请求积分排序。</CardDescription></CardHeader><CardContent>
          {view.models.length ? <Table><TableHeader><TableRow><TableHead>模型</TableHead><TableHead className="text-right">请求积分</TableHead><TableHead className="text-right">计费积分</TableHead><TableHead className="text-right">调用次数</TableHead></TableRow></TableHeader><TableBody>
            {view.models.map((m) => <TableRow key={m.key}><TableCell className="break-all">{m.key}</TableCell><TableCell className="text-right tabular-nums">{format(m.credits)}</TableCell><TableCell className="text-right tabular-nums">{format(m.billableCredits)}</TableCell><TableCell className="text-right tabular-nums">{format(m.records)}</TableCell></TableRow>)}
          </TableBody></Table> : <Empty>所选范围暂无模型用量。</Empty>}
        </CardContent></Card>
        <p className="text-xs leading-5 text-muted-foreground">已扫描 {source.filesScanned} 个会话文件。{source.coverageStartAt ? `记录覆盖 ${time(source.coverageStartAt)} 至 ${time(source.coverageEndAt!)}。` : "尚未发现有效积分记录。"}{source.parseErrors > 0 ? `已跳过 ${source.parseErrors} 条无法读取或解析的记录。` : ""}</p>
      </> : <Empty>暂无可用的本地积分统计。</Empty>}
    </div> : <div className="space-y-8">
      <p className="text-sm leading-6 text-muted-foreground">额度来自账号查询的本地快照；同一账号的桌面端和 CLI 合并，国内版与国际版分开。每日消耗是相邻快照中同一资源额度已用量的增量，记录在后一次查询日期，仅代表观察到的消耗，不与本地请求积分相加。充值、到期和周期重置重新建立基线。</p>
      {!!quota?.errors.length && <Alert><CircleAlert /><AlertTitle>部分账号未能更新</AlertTitle><AlertDescription>{quota.errors.map((e) => e.message).filter((e, i, a) => a.indexOf(e) === i).join("；")}。下方保留上次成功查询的快照，请留意更新时间。</AlertDescription></Alert>}
      <Select value={account} onValueChange={setAccount}><SelectTrigger className="w-full sm:w-64" aria-label="筛选账号"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">全部账号</SelectItem>{allAccounts.map((a) => <SelectItem key={a.accountKey} value={a.accountKey}>{a.name}</SelectItem>)}</SelectContent></Select>
      <Metrics values={[["最近查询剩余额度", accounts.length ? accounts.reduce((n, a) => n + a.remaining, 0) : null], ["最近查询总额度", accounts.length ? accounts.reduce((n, a) => n + a.total, 0) : null], ["最近查询已用额度", accounts.length ? accounts.reduce((n, a) => n + a.used, 0) : null], ["已有快照账号", accounts.length]]} />
      <Card className="shadow-none"><CardHeader><CardTitle>每日观察消耗</CardTitle><CardDescription>本地保留近 {quota?.retentionDays ?? 90} 天快照。需要至少两次查询才能观察增量，空白不代表零消耗。</CardDescription></CardHeader><CardContent>{chart(accountDaily, ["credits"])}</CardContent></Card>
      <Card className="shadow-none"><CardHeader><CardTitle>账号额度</CardTitle><CardDescription>以下为最近查询值，不受上方时间范围影响。</CardDescription></CardHeader><CardContent>
        {accounts.length ? <Table><TableHeader><TableRow><TableHead>账号</TableHead><TableHead className="text-right">剩余</TableHead><TableHead className="text-right">已用 / 总额</TableHead><TableHead>查询时间</TableHead></TableRow></TableHeader><TableBody>
          {accounts.map((a) => <TableRow key={a.accountKey}><TableCell>{a.name}</TableCell><TableCell className="text-right tabular-nums">{format(a.remaining)}</TableCell><TableCell className="text-right tabular-nums">{format(a.used)} / {format(a.total)}</TableCell><TableCell className="whitespace-nowrap text-muted-foreground">{time(a.updatedAt)}</TableCell></TableRow>)}
        </TableBody></Table> : <Empty>{refreshing ? "正在读取账号快照…" : "暂无账号额度快照。添加账号后点击刷新统计，或在账号页查询积分，即可开始记录。"}</Empty>}
      </CardContent></Card>
    </div>}
    <Dialog open={detail} onOpenChange={setDetail}><DialogContent className="flex max-h-[85vh] min-w-0 flex-col sm:max-w-5xl"><DialogHeader><DialogTitle>Qoder 请求明细</DialogTitle><DialogDescription>展示本机最近 500 条积分记录中符合筛选的请求；图表使用完整历史。请求未提供账号标识。</DialogDescription></DialogHeader>
      <div className="min-h-0 overflow-auto"><Table><TableHeader><TableRow><TableHead>时间</TableHead><TableHead>模型 / 项目</TableHead><TableHead className="text-right">积分</TableHead><TableHead>计费状态</TableHead></TableRow></TableHeader><TableBody>
        {view?.requests.slice(page * 50, (page + 1) * 50).map((r, i) => <TableRow key={`${r.timestamp}-${page}-${i}`}><TableCell className="whitespace-nowrap">{time(r.timestamp)}</TableCell><TableCell><div>{r.model}</div><div className="text-xs text-muted-foreground">{r.project}</div></TableCell><TableCell className="text-right tabular-nums">{format(r.credits)}</TableCell><TableCell>{r.billable === true ? "计费" : r.billable === false ? "非计费" : "未提供"}</TableCell></TableRow>)}
      </TableBody></Table></div><div className="flex items-center justify-end gap-3 text-sm"><span>{view?.requests.length ?? 0} 条 · 第 {page + 1} 页</span><Button size="sm" variant="outline" disabled={page === 0} onClick={() => setPage((p) => p - 1)}>上一页</Button><Button size="sm" variant="outline" disabled={(page + 1) * 50 >= (view?.requests.length ?? 0)} onClick={() => setPage((p) => p + 1)}>下一页</Button></div>
    </DialogContent></Dialog>
  </div>;
}
