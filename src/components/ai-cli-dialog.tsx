import { useState } from "react";
import { Check, Copy, FileText, Terminal } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAccountsStore } from "@/stores/accounts";
import { useQoderView } from "@/stores/qoder-view";
import type { PlatformId } from "@/lib/platforms";
import { cliQueryCommands, cliQueryText, cliSkillGuide, type CliRegion, type CliTarget } from "@/lib/ai-cli-guide";

function CommandRow({ title, command }: { title: string; command: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
    } catch {
      toast.error("无法访问剪贴板，请选择下方命令手动复制");
    }
  }
  return <div className="min-w-0 rounded-xl border bg-card p-3 sm:p-4">
    <div className="mb-2 flex items-center justify-between gap-3">
      <p className="text-sm font-medium">{title}</p>
      <Button variant="ghost" size="sm" aria-label={`复制${title}命令`} onClick={() => void copy()} onBlur={() => setCopied(false)}>
        {copied ? <Check className="text-primary" /> : <Copy />}<span aria-live="polite">{copied ? "已复制" : "复制"}</span>
      </Button>
    </div>
    <code className="block select-text break-all font-mono text-xs leading-6 text-muted-foreground">{command}</code>
  </div>;
}

/** Copyable discovery/query examples only; opening this guide never runs a command. */
export function AiCliDialog({ platform }: { platform: PlatformId }) {
  const qoderVariant = useQoderView(s => s.variant);
  const codebuddyVariant = useAccountsStore(s => s.variant);
  const [target, setTarget] = useState<CliTarget>(platform === "qoder" ? "qoder-cli" : "codebuddy-cli");
  const [region, setRegion] = useState<CliRegion>(platform === "qoder" ? qoderVariant : codebuddyVariant);
  const [copied, setCopied] = useState<string | null>(null);
  const [showGuide, setShowGuide] = useState(false);
  const qoder = target !== "codebuddy-cli";
  const commands = cliQueryCommands(target, region);
  const guide = cliSkillGuide(target, region);
  const queryText = cliQueryText(target, region);
  async function copyBundle(kind: "guide" | "commands") {
    const text = kind === "guide" ? guide : queryText;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(text);
      toast.success(kind === "guide" ? "Skill 接入说明已复制，可直接发给人或 AI 编写 Skill" : "全部查询命令已复制");
    } catch {
      setCopied(null);
      setShowGuide(true);
      toast.error("无法访问剪贴板，请在接入说明中全选并手动复制");
    }
  }
  return <Dialog onOpenChange={open => {
    if (open) {
      setCopied(null);
      setShowGuide(false);
      setTarget(platform === "qoder" ? "qoder-cli" : "codebuddy-cli");
      setRegion(platform === "qoder" ? qoderVariant : codebuddyVariant);
    }
  }}>
    <DialogTrigger asChild>
      <button type="button" aria-label="AI CLI 调用指南" title="AI CLI 调用指南" className="group mb-4 flex w-full items-center gap-3 rounded-xl border border-sidebar-border bg-card p-2.5 text-left outline-none transition-colors hover:border-primary/40 focus-visible:ring-2 focus-visible:ring-ring md:p-3">
        <Terminal className="size-5 shrink-0 text-primary" />
        <span className="hidden min-w-0 md:block"><span className="block text-xs font-semibold">AI CLI</span><span className="mt-0.5 block text-[11px] text-muted-foreground">把操作交给你的 Agent</span></span>
      </button>
    </DialogTrigger>
    <DialogContent className="max-h-[85dvh] overflow-y-auto sm:max-w-2xl">
      <DialogHeader className="text-left">
        <p className="flex items-center gap-2 text-xs font-semibold tracking-wider text-primary"><Terminal className="size-4" />NEXT / AI CLI</p>
        <DialogTitle className="text-2xl">让 Agent 接入你的工作台</DialogTitle>
        <DialogDescription className="leading-6">一键复制接入说明，交给人或 AI 编写 Skill；也可以单独复制命令在终端使用。</DialogDescription>
      </DialogHeader>
      <div className="flex flex-wrap items-center justify-between gap-3 border-y py-3">
        <Select value={target} onValueChange={value => { setTarget(value as CliTarget); setCopied(null); }}>
          <SelectTrigger aria-label="CLI 目标客户端" className="w-full sm:w-48"><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem value="codebuddy-cli">CodeBuddy CLI</SelectItem><SelectItem value="qoder-cli">Qoder CLI</SelectItem><SelectItem value="qoder-desktop">Qoder 桌面端</SelectItem></SelectContent>
        </Select>
        <Tabs value={region} onValueChange={value => { setRegion(value as CliRegion); setCopied(null); }}><TabsList aria-label="CLI 区域"><TabsTrigger value="cn">国内版</TabsTrigger><TabsTrigger value="ai">国际版</TabsTrigger><TabsTrigger value="all">全部</TabsTrigger></TabsList></Tabs>
      </div>
      <section aria-label="批量复制" className="space-y-3 rounded-xl border border-primary/20 bg-primary/5 p-4">
        <div><p className="flex items-center gap-2 text-sm font-semibold"><FileText className="size-4 text-primary" />给 Skill 作者的一份完整说明</p><p className="mt-1 text-xs leading-6 text-muted-foreground">包含当前目标与区域、命令示例、参数、JSON 返回与错误处理，以及切号和自动任务的调用约定。</p></div>
        <div className="flex flex-wrap gap-2">
          <Button aria-label="复制 Skill 接入说明" onClick={() => void copyBundle("guide")}>{copied === guide ? <Check /> : <Copy />}<span aria-live="polite">{copied === guide ? "接入说明已复制" : "复制 Skill 接入说明"}</span></Button>
          <Button variant="outline" onClick={() => void copyBundle("commands")}>{copied === queryText ? <Check /> : <Copy />}{copied === queryText ? "查询命令已复制" : "复制全部查询命令"}</Button>
          <Button variant="ghost" size="sm" className="text-xs" aria-expanded={showGuide} aria-controls="cli-skill-guide" onClick={() => setShowGuide(value => !value)}>{showGuide ? "收起说明" : "预览说明"}</Button>
        </div>
        {showGuide && <textarea id="cli-skill-guide" aria-label="Skill 接入说明" readOnly value={guide} onFocus={event => event.currentTarget.select()} className="h-64 w-full resize-y rounded-lg border bg-card p-3 font-mono text-xs leading-6 outline-none focus-visible:ring-2 focus-visible:ring-ring" />}
      </section>
      <div className="grid min-w-0 gap-3">{commands.map(({ title, command }) => <CommandRow key={command} title={title} command={command} />)}</div>
      <p className="text-xs leading-6 text-muted-foreground">桌面安装包附带同版本命令；安装后请新开终端。stdout 为最终结果，stderr 为进度。{qoder ? "Qoder 的模型限额尚无可靠来源，返回未知；统计中的缺失计数不会当作零。" : "限额来自已有观察记录，未观察到限制不代表模型可用。"}</p>
    </DialogContent>
  </Dialog>;
}
