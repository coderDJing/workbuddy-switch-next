# MiniMax Code 接入可行性：本机 Windows 核对

日期：2026-11-11。只描述本机 Windows 桌面端与公开源码，不代表 macOS、国际版或其它版本的结论。
本次核对全程只读：未启动 MiniMax Code、未触发登录或令牌刷新、未改写任何凭据文件。

## 结论

可以接，且实现难度低于 Qoder。原因是三件事同时成立：

1. 凭据是**明文 JSON**（不像 Qoder 的 `v10` / DPAPI 密文），整组文件拷贝即可切换。
2. 每个区域只有**一个凭据槽**，桌面端与 CLI 共用同一份登录态，不需要像 Qoder 那样按客户端分开维护。
3. 官方 CLI 已开源（`MiniMax-AI/minimax-code`），设备码授权、文件存储、跨进程锁、代际约定、额度接口都能直接对照实现，不必靠反编译猜。

## 本机只读核对到的事实

### 安装与归属

- 桌面端：MiniMax Code 3.1.13，`D:\Program Files\minimax code\MiniMax Code.exe`（Electron，用户数据在 `%APPDATA%\MiniMax`）。
- CLI：**未安装**（全局 npm 无 `@minimax-ai/code`，无 `~/.minimax-code`，PATH 无 `mcode`）。
- 但 `~/.minimax/auth/prod/cn/mcode-public/` 已有完整凭据，且 `~/.minimax/v2/sessions` 有桌面端会话 → 这份登录态是**桌面端**写的，桌面端与 CLI 共用 `~/.minimax`。多账号切换只需处理一个目标。

### 凭据布局（字段名来自本机文件，值不记录）

`~/.minimax/auth/{buildEnv}/{region}/mcode-public/`，本机为 `prod/cn`，全局版按源码为 `prod/en`：

| 文件 | 内容 |
| --- | --- |
| `auth.json` | `schemaVersion:1` + `records["com.minimax.mcode.oauth.prod.cn\0<sha256>"]`，记录内含 `accessToken`（`mmoat_` 前缀）、`refreshToken`（`mmort_` 前缀）、`tokenType:Bearer`、`clientId:mcode-public`、`scopes:[agent.default]`、`audience:agent-backend`、`expiresAtMs`、`generation`、`loginEpoch` |
| `auth-state.json` | `schemaVersion:2`、`status`、`storeKind:file`、`generation`、`expiresAtMs`、`buildEnv`、`region`；官方实现明确拒绝把令牌类字段写进此文件 |
| `auth.lock` | 跨进程锁；另有 `oauth-lease-protocol` 负责刷新租约 |

### 身份与用量

- 当前账号身份：`%APPDATA%\MiniMax\minimax-agent-cn-config.json` 的 `sharedUser`（`userID` / `realUserID` / `userName` / `subUserName` / `deviceID`），只在当前登录账号下有值。
- `auth.json` 内**没有**邮箱、手机号或 uid，换号后旧账号标签只能靠登录时拿到的身份或 API 查询。
- 用量：`~/.minimax/v2/sessions/YYYY/MM/DD/<session>/messages.jsonl` 每条消息带 `usage{input,output,cacheRead,cacheWrite,totalTokens,cost}` 和 `model`，可做 Token 统计；但 `manifest.json` 与消息里都**没有账号标识**，会话也不按账号隔离 —— 只能做全局 / 时间窗统计，不能按账号归因。
- 运行态数据库 `v2/sqlite/runtime-state.sqlite` 只有 `agents` 表，没有会话账本。

## 可直接对照的项目

### 1. `MiniMax-AI/minimax-code`（官方 CLI 开源，MIT）

最值得抄的是认证层，几乎就是为切换工具写好的：

- `packages/oauth-core/src/namespace.ts`：路径与记录键的推导 —— `auth/{buildEnv}/{region}/mcode-public`，`credentialKey.account = sha256(authHome + "\0" + clientId)`（base64url）。这解释了为什么每个区域只有一个槽。
- `packages/oauth-core/src/credential-store/file-store.ts`：`records[service\0account]` 的读写与整文件原子写。
- `packages/oauth-core/src/state-store.ts`：`auth-state` 的合法状态枚举、`generation` 与 `refresh{id,owner}` 事务、敏感键拒绝。
- `packages/oauth-core/src/auth-core.ts`：**关键不变量** —— `status` 解析时要求 `credential.generation === state.generation`，不一致直接判 `error`；`loginEpoch` 在刷新中保持不变、交互式登录时更换；`handleUnauthorized` 用 `generation` / `loginEpoch` 判断"要不要重试"。
- `packages/oauth-core/src/refresh-owner.ts` + `cross-process-lock.ts`：刷新所有权用 `pid + startedAtMs`，只允许"有证据"地丢弃授权。
- `packages/tui/src/auth/identity-storage.ts`：`dataDir/cli-auth/{buildEnv}/{region}/account-identity.json`，存 `realUserID` / `userEmail` / `userName` / `subUserName` / `tokenFingerprint` / `status`。
- `packages/tui/src/account/matrix-account-client.ts`：身份与 Token Plan 额度端点 —— `agent.minimaxi.com/matrix/api/v1/user/get_user_extra_info`、`/matrix/api/v1/commerce/get_membership_info`、`www.minimaxi.com/v1/api/user/info`、`/v1/api/openplatform/coding_plan/remains`、`/v1/api/user/renewal`。

设备码授权（可支撑"网页登录添加"，不必先装客户端）：

- `POST https://account.minimax.cn/oauth2/device/code`（国际版 `account.minimax.io`），表单：`client_id=mcode-public`、`scope=agent.default`、`audience=agent-backend`、`code_challenge` + `code_challenge_method=S256`。
- 轮询 `POST /oauth2/token`：`grant_type=urn:ietf:params:oauth:grant-type:device_code`、`client_id`、`code_verifier`，代码位兼容标准 `device_code` 与账号侧 `user_code` 两种轮询参数，并处理 `status=pending/slow_down/denied/expired` 与 `authorization_pending`。
- 刷新：`grant_type=refresh_token`、`client_id=mcode-public`、`scope=agent.default`、`audience=agent-backend`。
- 令牌响应可选 `sub` / `account_id`（JWT claims），能解出来时可直接作为稳定账号 ID。

### 2. `VICqiufeeng/dsh-minimax-connect`（MIT，0.3.0）

一个已实测的第三方消费方，价值在"与官方客户端共存"的细节：

- 认 `~/.minimax/auth/prod/cn/mcode-public/auth.json`，复用同一把 `auth.lock` 与 `auth.json` / `auth-state.json` 代际约定；
- access token 约 1 小时，桌面端退出后无人续期，需提前约 5 分钟刷新、另有 60 秒巡检；
- `invalid_grant` 视为致命错，必须回客户端重新登录；
- 积分 `GET /minimax-cloud/api/v1/credit/details`、签到 `GET /signin/status?timezone_id=Asia/Shanghai` 与 `POST /signin/claim`；
- 明确只验证过 Windows + 国内版。

### 3. `anpingjing/free-minimax-models`

大陆区 OAuth 与思考档位的第三方实测记录，可作为设备码与模型侧的交叉参考（未逐条复核）。

### 4. 官方文档

`agent.minimax.io/docs/cli/*`、`platform.minimax.cn/docs/m-plan/minimax-code`：数据根目录 `~/.minimax`、`MINIMAX_DATA_DIR` 覆盖、profile 目录 `~/.minimax-<profile>`、`mcode login [--region global]`。

## 建议的接入设计（沿用 Qoder 模式）

- **目标与区域**：一个切换目标（桌面端与 CLI 同槽），区域 `cn` / `en`，账号库分区域。
- **账号包**：`auth.json` + `auth-state.json`（+ 可选 `cli-auth` 身份文件）+ 标签 + 身份键；凭据必须用本项目现有 AES-256-GCM 加密后落盘，不能像官方那样明文放。
- **切换**：关闭 `MiniMax Code.exe` → 同一把切换锁内备份现场 → 写两份文件 → 逐份 sha256 校验 → 任一步失败整组回滚 → 重开。写入时 `generation = max(现有) + 1`、换新 `loginEpoch`、`status=authenticated`、清掉 `refresh` / `authorization` 事务字段，且两份文件代际必须一致。
- **切出即回收**：桌面端会轮换 refresh token。切换前若现场凭据比账号包里的新，先把现场凭据写回该账号包，否则下次切回会 `invalid_grant`。这条是本项目"现场先保存再切换"的现成逻辑，直接复用。
- **身份**：优先用登录响应里的 `sub` / `account_id`，其次调 `get_user_extra_info` / `user/info`；当前账号可用 `sharedUser` 回显。查不到就允许用户手填标签，不猜。
- **积分与签到**：`credit/details`、`get_membership_info`、`coding_plan/remains`、`signin/*`，与 dsh 实测字段对齐；查询失败按错误处理，不按零积分。
- **Token 统计**：扫 `v2/sessions` 的 `messages.jsonl`，按日期 / 模型 / 项目聚合，界面明确标注不按账号归因。
- **CLI 可选增强**：`MINIMAX_DATA_DIR` / `--profile` 可让 CLI 用独立数据目录（`~/.minimax-<profile>`），理论上能"不打断桌面端"，但要改调用方式（wrapper / 别名的注入），不建议放进 v1。

## 风险与未验证

- **进程识别**：桌面端是 `MiniMax Code.exe`，但 CLI 是 Node 进程（`mcode` shim），不像 Qoder 有独立 exe，按映像名找进程不可靠；需要另找判据（`v2/runtime-owner-leases`、`auth.lock` 持有者、端口）才能做到"关闭运行中客户端"。
- **退出写回**：桌面端退出时可能把内存里的令牌写回磁盘，切换窗口必须落在进程完全退出之后（Qoder 已踩过这个坑）。
- **并发刷新互踢**：同一条 refresh token 被桌面端、CLI 和本工具三方刷新会互相作废，必须"先关后写"，并且本工具自己不做后台刷新。
- **官方自更新**：桌面端有 hot-update，凭据布局可能随版本变化；记录验证过的版本号，未知版本拒绝写入，而不是照着旧格式硬写。
- **明文凭据**：`auth.json` 是明文 token，账号库、导出备份、日志三条路径都不能落原文；AGENTS.md 的 RC 诊断约束同样适用。
- **未验证**：国际版（`prod/en`）、macOS、首次在空目录添加账号、真实登录与真实切换、桌面端 + CLI 同时在线、跨设备 refresh token 轮换行为。

## 工作量参考

- 认证与切换核心（含测试）：约 1.8k–2.5k 行 Rust。Qoder 同类模块 5.4k 行，但那里有四个目标、加密双格式和会话关联，这里单槽明文，能砍掉一半以上。
- 前端：约 0.8k–1.2k 行，新增平台枚举 / 页面 / 账号视图 / 登录弹窗，并改 `src/lib/platforms.ts`、`api.ts`、`App.tsx`、`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`。
- 文档：README「支持的工具」加一行与一节说明，补验证范围。

## 落地结果（2026-11-11 第一期）

按上面的设计与「账号包 + 切换 + 积分」范围实现完成，实际改动：

- 后端 `crates/wb-switch-core/src/modules/minimax/`：`paths` / `protocol` / `vault` / `proc` / `credits` / `oauth` / `mod`，含 18 项离线测试。
- 命令层：`get_minimax_status`、`minimax_action`（`src-tauri`），以及 `/api/minimax/status`、`/api/minimax/action`（webui）。
- 前端：平台注册表加 `minimax`，新增 `MinimaxPage`、`minimax-status` / `minimax-view` store、产品图标（取自安装目录 `uninstallerIcon.ico` 的 256px 帧）、演示数据与 `operations.json` 条目。

实测结论与设计一致，其中三条值得记下来：

1. **单槽假设成立**：本机只有国内版登录槽有文件，国际版 `configured=false`，状态与切换都按区域独立处理，不需要客户端维度。
2. **代际写入正确**：真实切换把 `generation` 从 1 抬到 2，`auth.json` 记录与 `auth-state.json` 同步，`refresh` 事务清空，读回校验通过，并留下加密备份。
3. **现场令牌不能代刷**：本机访问令牌已过期，按设计只报错并提示打开客户端；早期版本顺手把账号标成「登录已过期」，会让切换按钮变灰，现已改为只报错、不改账号状态。

另外补了一个设计期没考虑到的边界：**同一账号在客户端重新登录会换 `loginEpoch`**，只按代际匹配会多出一张只剩失效令牌的卡片，因此 upsert 追加了「区域 + 平台用户 id」的兜底匹配。

进程识别仍按桌面端映像名 + CLI 的 `@minimax-ai/code` 命令行匹配，未用官方 `runtime-owner-leases`；`auth.lock` 只作为跨进程互斥锁使用。国际版真实登录、macOS 与「桌面端 + CLI 同时在线」仍未实测。
