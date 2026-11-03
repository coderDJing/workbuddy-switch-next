import { useEffect, useRef, useState } from "react";
import { ExternalLink, Loader2 } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import type { QoderTarget } from "@/lib/types";
import { useQoderStatus } from "@/stores/qoder-status";

export function QoderLoginDialog({ open, onOpenChange, target, label }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  target: QoderTarget;
  label: string;
}) {
  const refresh = useQoderStatus((state) => state.refresh);
  const session = useRef<string | null>(null);
  const generation = useRef(0);
  const [loginId, setLoginId] = useState<string | null>(null);
  const [uri, setUri] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<{label: string; clients: QoderTarget[]; warnings: string[]} | null>(null);

  useEffect(() => {
    if (!open) return;
    setLoginId(null); setUri(""); setBusy(false); setError(""); setResult(null);
    return () => {
      generation.current += 1;
      const id = session.current;
      session.current = null;
      if (id) void api.cancelQoderLogin(id).catch(() => {});
    };
  }, [open, target]);

  useEffect(() => {
    if (!loginId || result || error) return;
    let stopped = false;
    let timer: number | undefined;
    async function poll() {
      try {
        const response = await api.pollQoderLogin(loginId!);
        if (stopped) return;
        if (response.done) {
          if (response.result) { setResult(response.result); void refresh(); }
          else setError(response.error || "登录失败，请重试");
          return;
        }
        timer = window.setTimeout(poll, 2000);
      } catch (cause) {
        if (!stopped) setError(api.asError(cause));
      }
    }
    void poll();
    return () => { stopped = true; if (timer !== undefined) window.clearTimeout(timer); };
  }, [loginId, result, error, refresh]);

  async function start() {
    const current = generation.current;
    setBusy(true); setError("");
    try {
      const response = await api.startQoderLogin(target);
      if (current !== generation.current) { void api.cancelQoderLogin(response.loginId); return; }
      session.current = response.loginId;
      setLoginId(response.loginId);
      setUri(response.verificationUri);
      try {
        if (api.isWebui()) window.open(response.verificationUri, "_blank", "noopener,noreferrer");
        else {
          const { openUrl } = await import("@tauri-apps/plugin-opener");
          await openUrl(response.verificationUri);
        }
      } catch {
        // The visible link remains available if automatic opening fails.
      }
    } catch (cause) {
      if (current === generation.current) setError(api.asError(cause));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>登录添加 {label} 账号</DialogTitle>
          <DialogDescription>在 Qoder 官方网页完成登录，添加账号后可选择切换桌面端或 CLI。</DialogDescription>
        </DialogHeader>
        {!loginId && !result && <Button onClick={start} disabled={busy}>{busy ? "正在发起登录…" : "打开 Qoder 登录"}</Button>}
        {loginId && !result && !error && <div className="space-y-3">
          <Alert><ExternalLink className="size-4" /><AlertDescription>请在浏览器完成授权。若窗口没有打开，可<a className="ml-1 text-primary underline" href={uri} target="_blank" rel="noreferrer">打开登录页面</a>。</AlertDescription></Alert>
          <p className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />等待登录完成…</p>
        </div>}
        {result && <Alert><AlertDescription>已添加账号：{result.label}。{result.warnings.map((warning) => <span key={warning} className="mt-1 block">{warning}</span>)}</AlertDescription></Alert>}
        {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}
        <DialogFooter><Button variant={result ? "default" : "outline"} onClick={() => onOpenChange(false)}>{result ? "完成" : "关闭"}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
