# AI 命令行：现有平台完整管理

一次性命令执行后退出，无需打开 GUI 或启动 webui。支持 `codebuddy-cli`（默认）、`workbuddy`、`codebuddy-ide`、`vscode`、`jetbrains`、`qoder-cli` 和 `qoder-desktop`。Qoder 使用独立账号库，不混入 WorkBuddy / CodeBuddy 账号。`help --json` 返回目标和快捷命令能力；`operations` 返回与 WebUI 共用的完整命名操作及参数契约。

## 覆盖范围

| 目标 | 状态 / 账号 / 积分 / 综合查询 / 切号 | Token / 积分统计 / 每日领取 | 自动任务与其他管理 |
| --- | --- | --- | --- |
| WorkBuddy、CodeBuddy IDE | 支持，国内版 / 国际版 | 统计为 WorkBuddy/CodeBuddy 平台汇总；每日签到按账号资格返回 | `call`：账号管理、会话复制 / 关联 / 同步、任务配置、导入导出、WebDAV |
| CodeBuddy CLI | 支持，保留已有默认行为 | 同上 | `limits` 保留已有观察台账；其他操作用 `call` |
| VS Code、JetBrains CodeBuddy 插件 | 支持，国内版 | 同上 | `call` 暴露各客户端已有能力；JetBrains 无会话复制能力 |
| Qoder 桌面端 / CLI | 支持，国内版 / 国际版 | 保留区域筛选、本地用量与额度快照口径 | 保留 `automation`；`call` 补充本机识别、账号管理、备份和 CLI Hooks 接入 |

桌面悬浮窗窗口管理、安装包下载重启、开机自启及原生权限窗口依赖桌面宿主，不伪装成独立 CLI 能力；`help --json` 的 `desktopHostOnly` 列出这些边界。后台调度仍需桌面应用或 `serve` 常驻，执行一次命令不会启动调度服务。

## WorkBuddy 与各 IDE / 插件

```bash
workbuddy-switch-next status --target workbuddy --variant cn
workbuddy-switch-next accounts --target codebuddy-ide --variant ai
workbuddy-switch-next inspect --target workbuddy --summary
workbuddy-switch-next switch ACCOUNT_ID --target workbuddy --dry-run
workbuddy-switch-next switch ACCOUNT_ID --target workbuddy
workbuddy-switch-next switch ACCOUNT_ID --target codebuddy-ide --close-running --relaunch
workbuddy-switch-next accounts --target vscode
workbuddy-switch-next status --target jetbrains
workbuddy-switch-next token-stats --target workbuddy
workbuddy-switch-next credit-stats --target codebuddy-cli
workbuddy-switch-next rewards ACCOUNT_ID --target workbuddy
```

桌面目标默认拒绝切换运行中的客户端。需要关闭并重开时，同时指定 `--close-running --relaunch`；这复用现有客户端重启事务，可能中断任务。`--dry-run` 只读状态与账号元数据，不验证凭据。CodeBuddy CLI 仍保留既有关闭 CLI、不重开的语义；Qoder 进程策略见下节。

WorkBuddy / CodeBuddy IDE 的 `status` 默认同时展示两区，`--variant` 可选单区；切换区域以账号本身为准。插件仅支持国内账号。`activeForTarget` 对应账号所在区域的客户端，多个区域状态放在 `clientStatus.clients` 中。完整模型可用性未知，桌面目标不提供快捷 `limits`，平台观察台账可用 `call get_rate_limits`。

WorkBuddy/CodeBuddy 的 `token-stats` 和 `credit-stats` 是平台汇总，不接受 `--variant`、账号筛选或 Qoder 专有统计参数；不把缺失数据当作零。`credit-stats --refresh` 可能刷新官方用量。`rewards` 是现有每日签到能力，必须指定账号或显式 `--all`，国际版没有签到时返回跳过，不宣称获得奖励。

## 完整命名操作

```bash
workbuddy-switch-next operations
workbuddy-switch-next operations update_account_display
workbuddy-switch-next call get_accounts
workbuddy-switch-next call update_account_display --input-file edit-account.json
workbuddy-switch-next call list_account_sessions --input-file sessions.json
workbuddy-switch-next call get_auto_checkin_config
workbuddy-switch-next call get_auto_rotate_config
workbuddy-switch-next call get_qoder_companion --input-file qoder-region.json
workbuddy-switch-next call webdav_info
workbuddy-switch-next call export_all_accounts --output-file accounts-backup.json
```

`edit-account.json` 示例：

```json
{"accountId":"ACCOUNT_ID","patch":{"note":"工作账号"}}
```

`sessions.json` 示例：

```json
{"accountId":"ACCOUNT_ID","client":"workbuddy"}
```

`qoder-region.json` 示例：

```json
{"global":false}
```

命名操作使用与界面一致的操作名和请求结构，在进程内调用同一后端，不访问网络端口、不要求服务已启动。`operations OPERATION` 返回 `parameters`（类型、必填性和枚举值）、输入表达式与前端函数签名；未知顶层字段、缺少必填字段、错误基本类型和错误枚举在业务执行前拒绝。GET 参数进行 URL 编解码，支持中文和带特殊字符的账号 ID。复杂会话结构与预览令牌继续由原有 core 校验。

`call` 不接受 `--target`、`--variant` 等快捷命令筛选参数：操作需要的客户端、区域和账号必须放进 JSON 输入。`--input-file -` 从标准输入读取，支持 UTF-8 BOM，最多 64 MiB。密码、令牌和备份正文放文件或 stdin，不放进命令行参数。JSON stdout 使用统一结果信封，实际结果位于 `result`；普通结果隐藏凭据，失败不输出原始远端响应。

`export_accounts`、`export_all_accounts`、`export_qoder_accounts` 必须指定 `--output-file`，原始备份只写文件，stdout 仅报告路径和包含凭据标记。`--output-file` 不覆盖已有文件；执行失败时移除尚未完成的新文件。普通操作指定输出文件时写入脱敏后的业务结果。备份文件仍包含有效凭据，需妥善保管。

账号 CRUD、登录态导入、导入预览 / 导入、会话复制 / 关联 / 同步、签到 / 每日任务、自动领取 / 轮换配置、限额 Hooks、Qoder CLI Companion Hooks、WebDAV 配置 / 测试 / 备份 / 恢复 / 同步均可通过 `operations` 发现。会话同步和备份恢复应先获取预览，再原样携带对应的 `previewToken` 或快照标识执行，遵循现有冲突与锁定规则。

命名切号操作默认不关闭客户端，需在 JSON 中显式指定 `restart:true`（Qoder 用 `closeRunning` / `relaunch`）才允许对应进程动作。更推荐使用带 `--dry-run` 和明确进程策略的 `switch --target` 快捷命令。

## 登录添加

```bash
workbuddy-switch-next login --target workbuddy --variant cn
workbuddy-switch-next login --target qoder-cli --variant ai
```

命令向 stderr 输出官方授权链接并在同一进程等待授权完成，成功后保存账号，不切换客户端；无需另外启动界面。WorkBuddy / CodeBuddy 各端复用共同账号登录流程，Qoder 使用原有区域设备授权流程。不要把登录拆成跨进程的 `oauth_start` / `oauth_poll` 调用：这些状态仅存在于发起进程，相关操作在目录中标为 `oneShot:false`，用 `login` 完成完整流程。`switch_progress` 同样不提供跨进程任务状态；切号命令会等待事务结束。

路由与输入契约维护在 `src/lib/operations.json`，前端和 CLI 共用。修改前端 API 参数后运行 `node scripts/sync-cli-operations.mjs --write`，验证时运行 `node scripts/sync-cli-operations.mjs`，避免 CLI 参数说明与实际实现漂移。

## 一键复制给 Skill 作者

打开界面侧栏的「AI CLI」，选择客户端和区域，点击「复制 Skill 接入说明」。复制内容是一份完整 Markdown，包含来源版本、能力发现、查询与操作示例、JSON 字段、退出码、数据边界和操作约定，可直接交给人或 AI 编写 Skill。内容随目标和区域变化，不包含实际账号、凭据或本机私有路径；安装版本与能力仍以实际 CLI 返回为准。

只需命令时，可用「复制全部查询命令」或各行的「复制」。预览说明可以手动全选复制；剪贴板不可用时会自动展开预览。复制和预览均不执行命令，也不会创建或安装 Skill。

## Qoder 调用

```bash
workbuddy-switch-next accounts --target qoder-cli --variant cn
workbuddy-switch-next inspect --target qoder-cli --variant cn --summary
workbuddy-switch-next status --target qoder-cli --variant ai
workbuddy-switch-next credits --target qoder-cli --variant all --summary
workbuddy-switch-next switch <ACCOUNT_ID> --target qoder-cli --dry-run
workbuddy-switch-next switch <ACCOUNT_ID> --target qoder-cli
workbuddy-switch-next token-stats --target qoder-cli --variant cn
workbuddy-switch-next credit-stats --target qoder-cli --variant cn
workbuddy-switch-next credit-stats --target qoder-cli --variant cn --refresh
workbuddy-switch-next rewards <ACCOUNT_ID> --target qoder-cli
workbuddy-switch-next automation get --target qoder-cli --variant cn
workbuddy-switch-next automation run --target qoder-cli --variant cn
```

- `--variant cn|ai|all` 默认 all。账号稳定 ID 由 `accounts` 返回；同一区域、同一身份的桌面端与 CLI 合并，余额只查询一次。也接受完整账号包 ID，按该身份选择目标客户端最新保存的包；不按昵称或 ID 前缀猜测。`clientPrepared` 只表示存在对应客户端账号包，不代表凭据已经验证有效。
- `status` 的 `clients` 数组覆盖所选区域，每项含 `activeAccountId`、`activeAccountName`、运行和登录状态。`accounts` / `inspect` 的 `activeForTarget` 表示是否为对应区域目标客户端的当前账号。`inspect` 在查询后重读状态；失败时保留查询结果，当前标记为 null。
- Qoder 切换默认拒绝运行中的目标。显式 `--close-running` 才允许关闭；桌面端还可加 `--relaunch`，仅重开切换前已运行的桌面端，CLI 不自动重开。`--dry-run` 只输出进程与账号包元数据，不验证凭据、不写登录状态。实际切换使用 Qoder 的专用锁、备份及回滚流程；过程不可中途强制终止。
- `credits` / `inspect` 复用统一的积分汇总、`*Display` 两位展示值、逐账号错误和部分成功退出码。`--summary` 省略套餐、附加包和专属积分明细，保留原始精度、到期时间、失败信息与当前账号。Qoder 暂无可靠模型限额来源，`inspect` 固定返回 `availabilityKnown=false`、`limitsSupported=false`；`limits` 返回 `UNSUPPORTED_CAPABILITY`，退出码 2，不伪造恢复时间。`--model` 不适用于 Qoder。
- `token-stats` 返回所选区域的本地会话累计统计，零占位或缺失 Token 计数保持 unavailable。`credit-stats` 分别返回 `localCredits` 与 `snapshots`：本地计费/非计费请求积分、账号额度快照的观察消耗不可相加。请求历史缺少可靠账号归属，`accountAttributionAvailable=false`。默认不联网；`--refresh` 只查询所选区域，支持 `--timeout` 与 `--concurrency`，失败保留本地统计并返回非零退出码。
- `rewards` 必须指定账号或显式 `--all`，逐账号顺序领取，沿用每日任务组件的领取流程，不自动切号。完整安装需带 daily-runner；缺少组件、资格不足、授权失效等作为失败返回，不把已尝试视为已到账。
- `automation get` 查看配置及记录；`automation run` 只执行所选区域已启用且到期的任务，不绕过每天次数、重试间隔及轮换空闲检查，也不启动常驻调度。持续调度需运行桌面应用或 `serve`。发现任务占锁会明确报错；正常中断会停止安排后续账号，已开始的任务完成后返回。

修改自动任务配置时，将区域补丁保存为 JSON 文件，例如 `qoder-settings.json`：

```json
{"dailyEnabled":true,"dailyTime":"09:00","alertsEnabled":true,"expiryDays":7}
```

```bash
workbuddy-switch-next automation set --target qoder-cli --variant cn --settings-file qoder-settings.json
```

支持字段：`dailyEnabled`、`dailyTime`、`alertsEnabled`、`expiryDays`、`lowBalance`、`rotateEnabled`、`rotateBelow`、`cooldownMinutes`。只修改明确提供的字段，保留其他配置及另一地区；未知字段和非法数值拒绝写入。时间为本机时区。`automation set` 必须明确 cn 或 ai，不能使用 all。

以下模型限额、共享账号库、CodeBuddy 认证更新与默认关闭进程的说明适用于 `codebuddy-cli`；通用编码、JSON 信封和中断退出约定同样适用于 Qoder。Qoder 的 `--text` 为易读缩进 JSON，不套用 CodeBuddy 专用状态文案。

## MiniMax Code 调用

桌面端与 CLI 共用 `~/.minimax/auth/prod/{cn|en}/mcode-public/` 的一份登录态，因此只有一个目标 `minimax-desktop`，用 `--variant cn|ai` 选区域（`ai` 即凭据目录里的 `en`）。

```bash
workbuddy-switch-next accounts --target minimax-desktop
workbuddy-switch-next status --target minimax-desktop --variant cn
workbuddy-switch-next inspect --target minimax-desktop --variant ai
workbuddy-switch-next credits --target minimax-desktop --timeout 20
workbuddy-switch-next switch <ACCOUNT_ID> --target minimax-desktop --dry-run
workbuddy-switch-next switch <ACCOUNT_ID> --target minimax-desktop --close-running --relaunch
workbuddy-switch-next login --target minimax-desktop --variant cn
```

- `login` 走 OAuth 设备码流程：先把 8 位用户码和授权地址写到 stderr（JSON 模式下是 `event=authorization_required`），再在当前进程内轮询到授权完成。成功返回 `status=authorized` 与新账号元数据；**只入库，不写现场登录、不切客户端**。`device_code` 与 PKCE `verifier` 留在进程内，不落盘。
- 返回值里 `sharedLogin=true` 表示桌面端与 CLI 共用一个登录槽，`nativeCliSupported=true` 表示该账号可被原生 CLI 使用；`clients` 覆盖所选区域的全部客户端。
- 支持 `accounts` / `status` / `inspect` / `credits` / `switch` / `login` / `rewards` / `token-stats` / `credit-stats` / `automation`。`limits` 返回 `UNSUPPORTED_CAPABILITY`（退出码 2）。Token 统计为本机所有区域与账号合计；credit-stats 按区域保存快照。rewards 必须指定账号或 --all，领取后回查确认。automation get/set/run 按区域管理已启用任务。`--model` / `--concurrency` / `--wait` 不适用。
- `credits` 支持 `--timeout`（默认 30 秒，超出按超时失败）。现场账号的令牌过期时报错并提示打开客户端刷新，不代刷；非现场账号会先用 refresh token 换新再查询。

## 命令

| 命令 | 用途 |
| --- | --- |
| `inspect [ACCOUNT_ID] --summary` | 一次查询账号、CLI 状态、新鲜积分、到期信息和模型限额 |
| `inspect [ACCOUNT_ID]` | 同上，保留完整账号和资源明细 |
| `accounts [list]` | 账号元数据与稳定 ID，不返回登录凭据 |
| `status --target codebuddy-cli` | 当前 CLI 账号及认证配置状态 |
| `credits [ACCOUNT_ID \| --all] [--summary]` | 查询积分；不指定账号时查询全部 |
| `limits [ACCOUNT_ID]` | 读取观察到的模型限制与恢复时间 |
| `switch ACCOUNT_ID [--dry-run]` | 切号或预览影响范围 |
| `help [--json]` / `version [--json]` | 帮助或版本 |
| `serve [--port PORT] [--no-open]` | 持续运行 webui；不带命令也启动 webui |

优先使用返回的账号 `id`。UID 只有唯一匹配时可用，昵称不作为账号选择器。不带 `--target` 的旧 `status` 仍查询 WorkBuddy。

## 参数

| 参数 | 适用范围与默认值 |
| --- | --- |
| `--summary` | inspect/credits：紧凑结果，不增加请求 |
| `--text` / `--json` / `--ascii-json` | 一次性命令的输出格式；text 不可与 JSON 参数混用 |
| `--target TARGET` | codebuddy-cli（默认）、qoder-cli、qoder-desktop |
| `--variant cn\|ai\|all` | accounts/inspect/credits/limits/switch，默认 all；旧 status 只接受 cn/ai |
| `--model MODEL` | inspect/limits，按模型名精确筛选 |
| `--timeout SECONDS` | inspect/credits，每账号默认 30 秒，范围 1–3600 |
| `--concurrency N` | inspect/credits，默认 4，范围 1–16 |
| `--wait SECONDS` | 查询/切号等待认证锁，默认 30 秒，范围 0–3600；0 为立即失败 |
| `--quiet` | 关闭 CLI 进度输出 |
| `--dry-run` | 只用于 switch；不写配置、不关闭进程，也不验证凭据 |

新查询与切号命令默认 JSON；help、version 和旧 WorkBuddy status 默认文本。`-h/--help`、`-V/--version` 是帮助和版本别名。

## 返回结果

stdout 为一个最终 JSON 对象，stderr 为进度和诊断；不要合并后解析。等待期间每 2 秒有心跳，逐账号查询会报告完成计数。JSON 进度为 JSON Lines，底层诊断也可能是文本。

公共字段：`schemaVersion`、`ok`、`command`、`target`、`scope`、`elapsedMs`、`exitCode`、`error`。成功时 error 为 null，失败时为 `{code,message,retryable,hint}`，逐账号查询错误使用相同结构。retryable 的 true/false/null 分别表示可重试、不宜直接重试、无法可靠判断。

scope 为 account（账号/积分/奖励）、client（客户端状态/切号/限额观察）、mixed（inspect）或 platform（Qoder 统计与自动任务）；help/version 的 scope、target 为 null。CodeBuddy 优先读取 `clientStatus`、`activeAccountId`、`activeForTarget`；早期别名 `codebuddyCli`、`activeCliAccountId`、`activeCli` 仍保留。Qoder 当前状态按区域放在 `clients` 数组中。

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功，空账号列表也算成功 |
| 1 | 操作失败，或全部积分查询失败 |
| 2 | 参数或账号选择错误 |
| 3 | 部分结果，已成功的数据保留 |
| 130 | 正常中断，执行中的认证事务已完成 |

inspect 最后的状态读取失败时保留积分与限额，以 `clientStatusError` 说明错误；状态未验证的当前标记为 null。可以单独重试 status，无需重新查询全部积分。

## 积分与到期

- `summary.complete` 表示积分查询是否全部成功；`balanceComplete` 还要求成功账号的额度明确。失败账号不计入总量。
- `balanceType` 区分 finite/unlimited/unknown。不限量账号 `unlimited=true`，余额/容量为 null；未知值也不能当成零。
- 汇总包含 `unlimitedAccounts`、`unknownBalanceAccounts` 及已知有限小计 `finiteTotalRemaining/finiteTotalCapacity`。存在不限量或未知额度时，整体总量为 null。
- 原始数值用于计算；`*Display` 是两位小数展示字符串，不表示官方最小积分单位。
- summary 省略独立联系字段和完整 resources，名称仍遵循账号展示设置。到期分组只保留正余额，排除已过期明细，未来和未知到期的条目不设数量上限。
- `expiryBalancesAsOf` 是摘要过滤的 Unix 毫秒时间。`expiryKnown=false` 表示到期时间未知；`soonestExpireAt/soonestExpireAtUtc` 是保留明细中最近的已知未来到期时间，无此时间时为 null。

摘要不重算官方余额，到期分组之和可能不同于原始总余额。`queryStartedAt/queriedAt`、积分 updatedAt、limitsScannedAt 表示各项时间，整份 inspect 并非原子快照。查询期间删除的账号保留本次结果并标记 present=false。

## 模型限额

limits 扫描最近 2 天的本机 CodeBuddy CLI 日志，并读取已落盘的共享账号 hook 台账，不消费事件队列、不安装 hooks、不扫描其他客户端日志。台账可能包含其他客户端捕获的同账号限额；未落盘的 hook 信号可能暂不可见。

条目包含 `resetAt`（Unix 毫秒）、`resetAtUtc`（UTC ISO 时间）、`remainingSeconds`，以及首次观察时间 observedAt、实际来源 source/sources/sourceClients、reason/evidence。历史记录未保存的来源信息不会猜填。

`modelStates.status=limited` 表示有活动限额证据；`--model` 指定但没有活动证据时为 unknown。未列出的模型、已到恢复时间的模型也仍是未知，不能据此证明调用可用。model=null 的 scope 为 unknown，不推断账号级限制；`unattributedLimitCount` 在模型筛选后仍保留。工具不主动调用模型探测可用性。

## 切号与并发

实际切号先关闭正在运行的 CodeBuddy CLI，再切换账号，不自动重开；不复制、删除或隔离本地对话。Windows 会关闭进程树。

dry-run 的 impact 列出 PID、可获取的启动时间、匹配数量和可能受影响的后代。任务是否忙碌为 unknown；这是时间快照，实际切号会重新扫描。进程查询失败返回 PROCESS_SCAN_FAILED，不会当成无会话继续写入。

真实结果中 `autoReopened=false`；关闭计数与 closeAttemptedProcessCount 表示尝试数，`processTerminationVerified=false`，不是逐进程退出证明。当前未生成持久备份，`backupCreated=false`、backupPath/backupId=null，内存回滚不能当作磁盘备份。

配套新版 GUI 与 CLI 共用跨进程认证锁，并同步账号状态与积分缓存。旧发行版不支持这套协议，同时写入须完全退出旧 GUI。遇到竞争时 CLI 自动等待，超时返回 AUTH_STATE_BUSY；不要删除锁文件或杀掉刷新进程。积分查询可能刷新凭据、写缓存，不是严格只读。

带历史「需重新登录」标记的账号再次请求成功时，会自动复核续期；续期成功后清除旧标记并同步界面。失败不丢弃已成功的查询结果，同一账号的失败复核至少间隔 60 秒；正常账号不增加复核请求。

正常 Ctrl+C/Windows Ctrl+Break/Unix SIGTERM 会停止排队，等待已开始的认证更新或切号完成，再以 130 返回。已完成结果保留，operationOk 表示操作结果；重试中断的切号前先确认当前账号。超时清理也可能使耗时超过 --timeout。强制终止、SIGKILL 和断电不受正常中断协议保护。

## 编码

stdout/stderr 使用无 BOM 的 UTF-8。AI 子进程按 UTF-8 解码；旧 PowerShell 5.1 管道可用 `--ascii-json`，标准 Unicode 转义在 JSON 解析后恢复原文，底层文本诊断不受此参数控制。

PowerShell 5.1 的 `>` 会将文本重新编码为 UTF-16LE；可用 `Out-File -Encoding utf8` 保存（5.1 带 BOM），或使用 PowerShell 7.4+ 原生重定向保留原始字节。

## 构建与维护

```bash
npm ci
npm run build
cargo build -p wb-switch-server --release
```

产物为 target/release/wb-switch（Windows 为 wb-switch.exe），Rust 需 1.89 或更新版本。测试使用 `WB_SWITCH_HOME` 隔离账号库与 CLI 配置；三平台 CI 包含 CLI 子进程与 npm 入口，本地通过不能代替跨平台 CI。

CLI 模块位于 crates/wb-switch-server/src/cli/，客户端操作集中在 Target 分派，账号与余额服务保持共享。core 不依赖 Tauri。认证锁覆盖 token 轮换和本地提交，当前按账号库串行化；账号库损坏时拒绝覆盖，缓存与统计拒绝迟到结果。并发锁、OAuth 保存和字段兼容的细节以 core 实现及测试为准。

## Next 分发与旧命令

桌面安装包自带同版本 CLI，命令为 `workbuddy-switch-next`，与桌面程序共用账号库。

桌面安装包从 [正式版 Releases](https://github.com/coderDJing/workbuddy-switch-next/releases/latest) 下载；RC 位于 [preview 入口](https://github.com/coderDJing/workbuddy-switch-next/releases/tag/preview)。正式版与 RC 的桌面自动更新通道隔离，RC 转正式版需退出应用后手动安装，账号库继续沿用。

- Windows：安装器将安装目录追加到当前用户 PATH，卸载时只移除自己添加的条目。安装或更新后请新开终端。
- macOS：首次打开桌面应用时，将 CLI 保存到 `~/.local/share/workbuddy-switch-next/cli/`，并在 `~/.local/bin` 创建命令入口；为常见 shell 的用户配置添加带标记的 PATH 片段。请新开终端，应用更新后再次启动会同步 CLI。不会覆盖该目录里已有的同名命令。

如命令不可用，先用 Windows 的 `where.exe workbuddy-switch-next` 或 macOS 的 `command -v workbuddy-switch-next` 检查 PATH 和同名命令；安装器不会覆盖已有命令。桌面安装目录里的程序也可按完整路径调用。

遇到旧版本命令占用路径时，先确认命令来源并正常卸载旧版本，再使用 Next 的命令入口，不需要删除 `~/.wb-switch`。
