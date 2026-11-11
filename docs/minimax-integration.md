# MiniMax Code 接入

提供 MiniMax Code 国内版 / 国际版的账号识别、**工具内网页登录添加**、加密保存、重命名、删除、整组切换、官方积分查询与**身份补全**。账号库与 WorkBuddy / CodeBuddy、Qoder、TRAE 隔离。

MiniMax Code 的桌面端与 CLI **共用同一份登录态**：`~/.minimax/auth/prod/{cn|en}/mcode-public/` 下每个区域只有一个登录槽，因此平台只有一个切换目标，不存在「桌面端 / CLI 各切一份」的情况。

## 使用

打开本工具的 MiniMax Code 平台，选择国内版或国际版，两种添加方式任选：

- **登录添加**：点「登录添加」→ 弹窗显示 8 位用户码并自动打开浏览器授权页 → 在浏览器确认 → 账号入库。全程不用打开客户端，也**不会**切换客户端当前登录。授权页有 5 分钟时限，过期后服务端会提示「缺少有效的 user_code」，重新发起一次即可；直连链接失效时也可以打开授权页首页手动输入用户码。
- **识别本机账号**：在官方客户端登录后点「识别本机账号」，或在当前登录卡片上点「添加当前账号」。

已有账号的卡片会显示最近一次查询到的积分、套餐名与到期时间。

- 「关闭并切换」会先请求客户端正常退出（等待 20 秒），再成组写入两份凭据文件并重新打开；客户端拒绝退出时直接停止，不强制终止进程。
- 切换前会把现场登录补收进账号库，避免客户端退出时轮换掉 refresh token 导致该账号丢失。
- 额度查询逐账号进行：对**非现场**账号，令牌过期会用 refresh token 换新并回写账号包；对**现场**账号只读不刷（代刷会让客户端自己的会话失效），过期时提示打开客户端刷新。
- 早期版本登录时拿不到平台 uid，卡片名会退化成「MiniMax CN 账号 N」。这类卡片（`uid` 以 `epoch:` 开头）的菜单里会多一项「识别身份」：补上真实 uid / 昵称 / 邮箱，并把同一账号的重复卡片合并掉。

## 命令行

`--target minimax-desktop` 是专用目标，`--variant cn|ai` 选区域（`ai` = 凭据目录里的 `en`）。

```sh
wb-switch accounts --target minimax-desktop            # 账号列表 + 客户端状态
wb-switch status   --target minimax-desktop            # 同上，只看状态
wb-switch inspect  --target minimax-desktop            # 一次拿全：账号、额度、失败明细
wb-switch credits  --target minimax-desktop --timeout 20
wb-switch switch <account-id> --target minimax-desktop [--dry-run] [--close-running --relaunch]
wb-switch login    --target minimax-desktop --variant cn
```

`login` 会先把用户码与授权地址打到 stderr（JSON 模式下是 `type=progress` + `event=authorization_required`），然后在本进程内轮询到授权完成；成功返回 `status=authorized` 与新账号元数据，**不会**写入现场登录。

通用入口同样可用：

```sh
wb-switch call get_minimax_status
wb-switch call minimax_action --input-file body.json
```

```json
{"action":"capture","input":{"region":"cn"}}
```

`action` 支持 `capture`、`switch`、`credits`、`rename`、`delete`、`open`、`identity`、`login_start`、`login_poll`、`login_cancel`；`input` 接受 `region`（`cn` / `en`，缺省 `cn`）、`accountId`、`name`、`loginId`、`closeRunning`、`relaunch`、`dryRun`、`timeoutSeconds`，未知字段被拒绝。

## 存储和边界

- 只读写 `~/.minimax/auth/prod/{cn|en}/mcode-public/` 下的 `auth.json` 与 `auth-state.json`；不碰 `auth.lock`、`~/.minimax` 里的日志与项目数据。
- 官方客户端要求 `auth.json` 记录里的 `generation` 与 `auth-state.json` 的 `generation` 一致，否则把自己的登录判成 `error`。切换时两份文件同时写入 `generation = max(账号包, 现场, state) + 1`，清掉刷新事务（`refresh`）与临时授权字段，写完读回核验；不一致就恢复原文件。
- 登录添加同样按这套代际约定生成账号包（`generation = 1`），只是不写现场文件。
- 记录键 `{service}\0{account}` 与官方 `createAuthNamespace` 一致：`service = com.minimax.mcode.oauth.prod.{cn|en}`，`account = base64url(sha256("{dataDir}/auth\0mcode-public"))`。
- `~/.wb-switch/minimax/accounts.json` 里只存元数据与**加密后**的 `{auth, state}`：Windows 用 DPAPI，macOS 用 Keychain 派生密钥 + AES-256-GCM；明文令牌不落盘。
- 设备码授权的 `device_code` / `code_verifier` 只留在进程内存（`loginId` 是随机 UUID 索引），不落盘、不进日志、不返回给前端。
- 每次切换前把现场凭据加密备份到 `~/.wb-switch/minimax/backups/`（每个区域保留最近 10 份）。
- 账号按区域 + `loginEpoch` 识别；同一账号在客户端重新登录换了 `loginEpoch` 时按平台用户 id 覆盖旧记录，不会多出一张只有失效令牌的卡片。
- 积分与身份只访问 `agent.minimax.cn` / `agent.minimax.io` 的固定接口；失败时不回显响应正文与凭据。

## 接口实现要点

**设备码授权（RFC 8628 + PKCE S256）**

`POST {account_origin}/oauth2/device/code`，表单 `client_id=mcode-public`、`scope=agent.default`、`audience=agent-backend`、`code_challenge`、`code_challenge_method=S256`；返回 `device_code` / `user_code` / `verification_uri(_complete)` / `expires_in=300` / `interval=3`。之后轮询 `POST {account_origin}/oauth2/token`（`grant_type=urn:ietf:params:oauth:grant-type:device_code` + `code_verifier`），未授权时返回 `authorization_pending` / `slow_down` / `expired_token`。非法 challenge 会被直接拒绝（`valid S256 PKCE is required`）。

**身份接口与签名**

授权响应通常没有 `sub`，所以登录成功后用刚拿到的令牌问 `GET /v1/api/user/info`，取 `data.userInfo.realUserID`（昵称 / 邮箱同处）。这个接口不认裸 Bearer：官方客户端还要求一组固定查询参数与两个签名头，缺了统一返回 `statusInfo.code = 2`。

```text
yy          = md5( encodeURIComponent(pathname + search) + "_" + (body || "{}") + md5(ms) + "ooui" )
x-timestamp = floor(ms / 1000)
x-signature = md5( f"{x-timestamp}I*7Cf%WZ#S&%1RlZJ&C2{body}" )
```

查询参数：`device_platform=web`、`biz_id=3`、`app_id=3001`、`version_code=22201`、`unix`、`timezone_offset`、`sys_language`、`lang`、`device_id=0`、`user_id`、`client=desktop`、`region`、`is_desktop=1`。`/matrix/api/v1/user/get_user_extra_info` 用 POST（空对象），其余用 GET。算法逐字对齐官方桌面端产物（`transport.js`），实现见 `crates/wb-switch-core/src/modules/minimax/sign.rs`。

**状态读取的两次探测**

桌面端是同一个可执行文件，与区域无关，所以进程探测只做一次（Windows 上用 `tasklist /FI "IMAGENAME eq MiniMax Code.exe"` 只问目标映像，不再把整张进程表取回来过滤）；现场登录的两份 JSON 每个区域也只解析一次。这两处原来都做了两遍，`status` 从 1.6–2.4 秒降到 0.3 秒量级。查询本身失败仍然报错而不是当成「没运行」——切换路径宁可停下也不能在客户端还开着时硬写凭据。

## 验证记录

2026-11-11，本机（Windows，MiniMax Code 3.1.13 国内版）实测：

| 检查项 | 结果 |
| --- | --- |
| `get_minimax_status` 读取本机登录 | 国内版 `configured=true`、`detected=true`、身份 `coder` / `576404468350857218`；国际版未登录 `configured=false`，两区域互不影响 |
| 状态不泄露令牌 | 响应里没有 token 字段；`accounts.json` 里搜不到 `accessToken` / `refreshToken` / JWT 前缀 |
| `capture` 加密保存 | 账号包 1268 字节密文，DPAPI 加解密正常 |
| `switch` 写入代际 | `auth.json` 记录与 `auth-state.json` 的 `generation` 由 1 同步升到 2，`status=authenticated`，`refresh` 事务已清空，状态文件不含令牌字段 |
| 切换备份 | 生成 `cn-<时间戳>-<uuid>.enc` |
| 积分查询 | 现场访问令牌已过期，按设计拒绝代刷并提示打开客户端刷新（不误标「登录已过期」） |
| `login` 设备码授权 | 用户码 `K8R5-CW92` → 22 秒后 `status=authorized`，账号入库，未改动现场登录 |
| `login` 端到端（含身份） | 用户码 `X6LR-F9EJ` → 57 秒完成；返回的账号 id 与已有卡片相同、`uid=576404468350857218`、`name=coder`、`identityProbe=["jwt: not-a-jwt","user/info: ok"]`，账号库仍为 1 条——**登录当场解析身份并并入已有卡片，不再产生重复卡** |
| `login` 超时与取消 | 用户码 `F4KT-VTLK` 300 秒未确认 → `status=failed`、退出码 3、账号库无新增、无残留会话 |
| 新账号可用性 | 用入库令牌查额度成功：`total=400`、`used=400`、到期 2026-12-09 |
| 身份补全 | `identity` 动作返回真实 uid `576404468350857218` 与昵称 `coder`，`identityProbe` 显示 `user/info: ok` |
| 重复卡合并 | 同 uid 的两张卡片合并为一张，优先保留现场那张，`mergedDuplicates=1` |
| CLI 目标 | `accounts/status/inspect/credits/switch` 均可跑通；`credits` 现场令牌过期时返回 `complete=false`、退出码 3 |
| `status` 耗时 | 去掉重复的现场解析与整表 `tasklist` 后，连续 4 次测得 316–776 ms（原 1638–2450 ms） |
| 进程识别 | 未运行时正确返回 `running=false`；用同名映像名验证 `tasklist /FI` 对含空格名称也能命中并解析出 PID |

离线测试 `cargo test -p wb-switch-core minimax` 覆盖 29 项：代际提升与事务清理、跨区域与残缺凭据拒绝、代际不一致检测、写两份文件并备份、关闭并重开、拒绝运行中客户端、同一账号重新登录覆盖旧记录、重复卡合并、签名与 MD5 向量、状态不泄露令牌等。

## 已知限制

- 国际版（`en`）本机未登录，登录、切换、积分与身份只用 `cn` 实测过；国际版的业务域名 `agent.minimax.io` 与签名逻辑按官方产物同构实现。
- macOS 未实测：账号包走 Keychain 派生密钥，路径与进程探测按平台分支实现。
- 现场访问令牌过期时不代刷，需要打开一次客户端；这是为了不打断客户端自己的刷新链路。
- 账号昵称 / 邮箱来自 `/v1/api/user/info`；若该接口临时不可用，登录仍会成功，只是卡片退化成「账号 N」，菜单里的「识别身份」可以事后补。
- 授权页有 5 分钟时限：过期或直连链接失效时，服务端会提示「缺少有效的 user_code」，重新发起一次即可。
- 「当前账号」先按代际精确匹配，代际对不上（例如刚刚重新登录过）再按平台 uid 认，避免界面同时出现「本机当前登录：尚未添加」和一张同名账号卡。**写入仍以代际为准**：这种情况下点切换会真正执行，把新令牌同步进客户端；登录结果里的 `matchesLiveClient` 会在弹窗里说明「这就是客户端当前登录的账号」。
- 客户端现场令牌过期时页面会显示一条琥珀色提示：本工具不代刷客户端的令牌，打开一次 MiniMax Code 让它自行刷新；账号包里那份新令牌仍可用于切换与积分查询。
- 没有 Token 统计、会话复制、自动轮换与悬浮窗接入：MiniMax Code 的本地用量数据格式尚未核实。
