import { useEffect, useRef, useState } from "react";
import { CheckCircle2, ExternalLink, Loader2 } from "lucide-react";

import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import * as api from "@/lib/api";
import { useMinimaxStatus } from "@/stores/minimax-status";

/** 在系统浏览器里打开授权页；WebUI 用新标签页，桌面端用 opener。 */
async function openExternal(url: string) {
  if (api.isWebui()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

/**
 * 工具内网页登录：设备码（RFC 8628）+ PKCE。
 *
 * 会话状态（`device_code` / `code_verifier`）留在后端进程内，前端只拿 `userCode` 与授权地址；
 * 授权成功后后端直接把凭据封进账号包，不写现场，因此这里只刷新列表、不切换客户端。
 */
export function MinimaxLoginDialog({ open, onOpenChange, region, label }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  region: api.MinimaxRegion;
  label: string;
}) {
  const refresh = useMinimaxStatus((state) => state.refresh);
  const session = useRef<string | null>(null);
  const generation = useRef(0);
  const [start, setStart] = useState<api.MinimaxLoginStart | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [account, setAccount] = useState<api.MinimaxAccount | null>(null);

  useEffect(() => {
    if (!open) return;
    setStart(null); setBusy(false); setError(""); setAccount(null);
    return () => {
      generation.current += 1;
      const id = session.current;
      session.current = null;
      if (id) void api.minimaxAction("login_cancel", { loginId: id }).catch(() => {});
    };
  }, [open, region]);

  useEffect(() => {
    if (!start || account || error) return;
    let stopped = false;
    let timer: number | undefined;
    async function poll() {
      try {
        const response = await api.minimaxAction("login_poll", { loginId: start!.loginId }) as api.MinimaxLoginPoll;
        if (stopped) return;
        if (response.status !== "pending") {
          if (response.status === "authorized") { setAccount(response.account); void refresh(); }
          else setError(response.error || "登录失败，请重试");
          return;
        }
        timer = window.setTimeout(poll, Math.max(1000, response.retryAfterMs ?? start!.intervalMs));
      } catch (cause) {
        if (!stopped) setError(api.asError(cause));
      }
    }
    timer = window.setTimeout(poll, Math.max(1000, start.intervalMs));
    return () => { stopped = true; if (timer !== undefined) window.clearTimeout(timer); };
  }, [start, account, error, refresh]);

  async function begin() {
    const current = generation.current;
    setBusy(true); setError("");
    try {
      const response = await api.minimaxAction("login_start", { region }) as api.MinimaxLoginStart;
      if (current !== generation.current) { void api.minimaxAction("login_cancel", { loginId: response.loginId }).catch(() => {}); return; }
      session.current = response.loginId;
      setStart(response);
      try {
        await openExternal(response.verificationUriComplete || response.verificationUri);
      } catch {
        // 页面里保留可点击的链接，自动打开失败也能继续。
      }
    } catch (cause) {
      if (current === generation.current) setError(api.asError(cause));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function openClient() {
    try { await api.minimaxAction("open", { region }); }
    catch (cause) { setError(api.asError(cause)); }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>登录添加 MiniMax Code {label}</DialogTitle>
          <DialogDescription>在浏览器完成授权后，账号会直接加密保存进账号库；不会立刻切换客户端。</DialogDescription>
        </DialogHeader>

        {!start && !account && <div className="space-y-3">
          <Button onClick={begin} disabled={busy}>{busy ? <><Loader2 className="animate-spin" />正在申请授权码…</> : "在浏览器中登录"}</Button>
          <p className="text-xs leading-5 text-muted-foreground">
            已有账号在客户端登录时，也可以不开浏览器：用「识别本机账号」直接保存当前登录。
            <button type="button" className="ml-1 cursor-pointer underline underline-offset-2 hover:text-foreground" onClick={() => void openClient()}>打开官方客户端</button>
          </p>
        </div>}

        {start && !account && !error && <div className="space-y-3">
          <Alert><ExternalLink className="size-4" /><AlertDescription>
            在浏览器打开的页面里确认下面的用户码：
            <span className="mt-2 block font-mono text-base font-semibold tracking-[0.18em] text-foreground">{start.userCode}</span>
            窗口没有打开时可<a className="ml-1 text-primary underline" href={start.verificationUriComplete || start.verificationUri} target="_blank" rel="noreferrer">打开授权页面</a>。
          </AlertDescription></Alert>
          <p className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />等待授权完成…（{Math.round(start.expiresIn / 60)} 分钟内有效）</p>
        </div>}

        {account && <Alert><CheckCircle2 className="size-4" /><AlertDescription>{account.matchesLiveClient
          ? `已添加账号：${account.name}。它就是客户端当前登录的账号，本次登录拿到的令牌已保存到账号库。`
          : `已添加账号：${account.name}。可在卡片上直接切换，或先改名。`}</AlertDescription></Alert>}
        {error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>}

        <DialogFooter>
          {error && <Button variant="outline" onClick={() => { setStart(null); setError(""); void begin(); }}>重试</Button>}
          <Button variant={account ? "default" : "outline"} onClick={() => onOpenChange(false)}>{account ? "完成" : "关闭"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
