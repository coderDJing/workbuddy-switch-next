# Daily task adapter

Pinned upstream code:

- WorkBuddy Daily, L0NE-6/WorkBuddy-Daily commit `90c7501c9b7e1996bd31c36388d46df5e89d001a` (v3.48), MIT; see `upstream/workbuddy/LICENSE` and `NOTICE.md`.
- Qoder Daily, L0NE-6/Qoder-Daily commit `1fb1ba04191157f241bfc1fc096b6f6a1c88096e` (v1.0.22), MIT; see `upstream/qoder/LICENSE` and `NOTICE.md`.

## WorkBuddy 集成补丁（4 处）

1. `ACCOUNTS = load_accounts() if __name__ == "__main__" else []`：宿主导入模块时不加载、不续期账号；凭据续期由宿主负责。
2. 新增模块级 `_task_is_claimable()`，内嵌的 `claimable()` 改为委托它：补领判定只看 `progress.current >= progress.target`，不依赖可能延迟或回滚的 `accept_status`。
3. 本机守护进程（daemon）的会话目录按当前凭据文件选择 `SESSION_DIRS[0]` 或 `[1]`，不再写死 `~/.workbuddy/sessions`（桌面版与国际版的区别）。上游 v3.48 已把 `websocket` 导入收敛到函数内部，我们不再需要额外删除它。
4. 摘要新增 `claimed` / `claimable` / `blocked`：宿主只有 `claimable == 0` 才判「今日已领完」；锁定、未达标、未下发的任务不会让按钮永远停在「未领完」。

配套改动：`tests/test_workbuddy_core.py` 的导入用例改为断言「导入不续期、显式 `load_accounts()` 才续期」；两个测试文件的 `load_functions()` 在装载 claim 相关函数时自动带上 `_task_is_claimable` / `_task_status_of` / `_as_int`。`tests/test_credits_report.py` 未随快照引入（依赖未 vendored 的 `credits_report.py`）。

## 上游变化（相对 v3.41 / v1.0.19）

- WorkBuddy v3.42–v3.48：签到重试持久化（失败轮次不会吞掉重试机会）、`skill_share` 自动完成、小程序 campaign 任务、库与 Buddy App 埋点修复、桌面鉴权与任务派发加固、星巴克 `app` 任务从「只提示」改为自动执行（agentos 会话 + `acp/bootstrap`）。
- Qoder v1.0.20–v1.0.22：国际版 claim 风控头修复、账号 2 工作流隔离、**claim 以额度回读是否增加判成功**——已受理但回读未确认时返回「…待确认」，适配层因此新增 `unconfirmed` 状态，不重试、不谎报成功。

## 依赖

`scripts/prepare-daily-runner.mjs` 固定安装 `requests==2.32.5`、`websocket-client==1.9.2`、`pyinstaller==6.16.0`。`websocket-client` 是 v3.48 起的新依赖（`cdp_ui_send` 的兜底通道）；漏装不会在导入时报错，只会在新账号走 UI 发送时 `ImportError`。

## 适配层

`runner.py` adapts their per-account task functions to the host's stdin/stdout protocol. The host owns refresh-token rotation and the runner receives access tokens only. Windows desktop tasks use the existing daemon when its current login matches the target account, otherwise they use the upstream event fallback; neither path replaces the desktop login or deletes session files. Daily tasks may take many minutes and can use credits, energy, or create conversations, as in the upstream projects.

跟进上游的流程与清单见 `docs/UPSTREAMS.md` 与 `docs/upstreams.json`。
