# TRAE 桌面端接入

本轮提供 TRAE 国内版 / 国际版 的账号识别、显式保存、重命名、删除、切换、官方额度查询与国内版每日签到。账号库与 WorkBuddy / CodeBuddy、Qoder 隔离。

原生 TRAE CLI 不在本轮范围。截至 2026-11-09，[官方 CLI 2.0 说明](https://docs.trae.cn/cli_about-trae-code-cli-2)限定企业旗舰版客户；个人桌面账号切换并不能获得这项权限。下文命令是本工具的管理 CLI，用来管理桌面账号。

## 使用

打开本工具的 TRAE 平台，选择国内版或国际版，点击“登录添加”，在官方客户端完成登录后返回添加账号。已有本机登录可直接点“识别本机账号”或当前登录卡片的“添加当前账号”。

页面沿用 WorkBuddy / Qoder 的账号工作台布局、紧凑偏好、更多菜单、行内重命名与切换确认。账号按区域与已验证用户 ID 识别，额度只展示一次。

```sh
workbuddy-switch-next accounts --target trae-desktop --variant cn
workbuddy-switch-next status --target trae-desktop --variant all
workbuddy-switch-next inspect --target trae-desktop --variant all
workbuddy-switch-next credits ACCOUNT_ID --target trae-desktop
workbuddy-switch-next rewards ACCOUNT_ID --target trae-desktop
workbuddy-switch-next rewards --all --target trae-desktop --variant cn
workbuddy-switch-next switch ACCOUNT_ID --target trae-desktop --dry-run
workbuddy-switch-next switch ACCOUNT_ID --target trae-desktop --close-running --relaunch
```

切换默认拒绝运行中的客户端，显式 `--close-running` 只请求正常退出，退出失败就停止；不强制终止进程。`--relaunch` 需要 `--close-running`。额度逐账号查询，`--timeout` 控制每账号网络请求超时；管理 CLI 还支持 `credit-stats`、`rewards` 和 `automation get/set/run`；不提供 Token 统计或原生 CLI 登录。`--summary` 对 TRAE 保留完整结果。部分额度请求失败返回退出码 3，未知余额不视为零。`rewards` 逐账号领取每日签到积分，只支持国内版账号；部分账号失败同样返回退出码 3。

完整管理操作通过 `operations trae_action` 发现，用 `call trae_action --input-file PATH` 调用。文件示例：

```json
{"action":"capture","input":{"variant":"cn","client":"desktop"}}
```

`action` 支持 `capture`、`switch`、`credits`、`checkin`、`rename`、`delete`、`open`；`input` 接受 `variant`、`client`、`accountId`、`name`、`closeRunning`、`relaunch`、`dryRun`、`timeoutSeconds`，未知字段被拒绝。`get_trae_status` 提供脱敏元数据。加密备份通过 `backup_export`、`backup_preview`、`backup_import` 操作导出和恢复，密码至少 8 位；恢复前必须预览并提交返回的 snapshotId。`checkin` 返回 `ok`、`message`、`authInvalid`、`retryable`、`credits` 与 `state`，`state` 取值为 `claimed`、`already`、`busy`、`closed`、`unconfirmed`、`auth`、`failed`。

## 存储和边界

- 读取各客户端 `User/globalStorage/storage.json`，校验 ByteCrypto 信封、内容摘要以及 Cloud IDE 用户 ID，区分账号中心 ID。
- 只替换 `iCubeAuthInfo://`、`iCubeServerData://`、`iCubeEntitlementInfo://` 登录字段，不修改 `state.vscdb`、项目、会话、机器标识。云端会话仍属于原账号；保留本地数据不代表能跨账号继续云端会话。
- `~/.wb-switch/trae/accounts.json` 中登录快照以 Windows DPAPI 或 macOS 独立 Keychain 密钥加密；绑定当前系统用户。查询状态不会读取 Keychain，显式保存、切换和额度查询可能触发系统授权。
- 切换前保存现场账号与加密回滚备份 `~/.wb-switch/trae/last-switch-backup.enc`。写入后读取核验，失败恢复原文件。文件写入使用同目录临时文件，macOS 权限为 `0600`。
- 不自行实施 OAuth 或刷新令牌；过期时在官方客户端重新登录并保存。额度只访问固定官方域名，禁止跟随重定向，不向快照中的任意主机发送凭据。
- 国内版额度请求官方 v2 `ide_user_ent_usage`（`require_usage` 与桌面 IDE 的 `req_source=1`）。v1 会省略计费标志，并只返回全 0 的旧字段。国际版仍使用 v1。解析保留小数，把 `credits_limit > 0` 的积分包相加（含 status 为 0 的未消耗包），排除隐藏、过期和加速包。旧格式或无法确定计费类型时返回未知，不将零填充字段当作可用额度。
- 不推断 TRAE / 豆包 / 原生 CLI 额度共享。

## 成熟项目参考与验证

协议与路径交叉参考 [cockpit-tools](https://github.com/jlcodes99/cockpit-tools/tree/02498ace91832248f0c6d43f6ea3894a2514aed7) 的 `trae_account_core_platform_storage`、`trae_account_core_import`、`trae_account_core_refresh` 和额度用例，以及 MIT 项目 [TraeWorkAssistant](https://github.com/smart-open/TraeWorkAssistant/tree/2b87e1ad81316696fdd7858e6f40aa466e68bfd2) 的客户端定位、Profile 和 TRAE 应用管理逻辑。这里只参考格式、常量和行为，独立实现适配层，没有整体复制第三方模块。

本机 Windows TRAE CN 已做只读验证：登录信封解码、摘要验证、JWT / 本地用户 ID 一致。国内版 v2 额度与官方客户端菜单一致，会计入 status 为 0 但仍有剩余的积分包；v1 的全 0 字段不能单独当成余额。隔离测试覆盖保存、切换、回滚、运行进程拒绝、关闭拒绝、跨区域拒绝、项目保留、损坏存储、凭据隐藏和管理 CLI 路由隔离。未在真实账号上执行切换；macOS 与国际版尚需对应客户端验收。

可在已登录 TRAE CN 的开发机显式运行只读检查（不保存账号、不切换客户端）：

```sh
cargo test -p wb-switch-core local_cn_readonly_smoke --lib -- --ignored --nocapture
```

## 每日签到

国内版 TRAE 提供官方每日签到积分，入口是 `/trae/api/v2/ug/checkin_credits/status` 与 `/claim`。协议对齐 MIT 项目 [Trae-AutoCheckin](https://github.com/L0NE-6/Trae-AutoCheckin/tree/d289f0d7c2a232936235df96bbffbbeaefcfc771) 的 `d289f0d`，这里只独立实现请求与状态判定，不整体复制第三方模块。

- 设备号只取账号快照里客户端注册过的那一个（`iCubeAuthInfo://icube-dc:<12–20 位数字>`）。生成的设备号会被服务端判成 9074，因此没有兜底生成；快照里没有设备号就不签，提示回官方客户端登录后重新识别保存。
- 签到按账号维度判定，官方限制一个设备号每天只能签一个账号，多账号需要各自的官方设备号。
- 只发小写 `x-device-id`，同时带上官方桌面端的 `X-User-Region`、`User-Agent`、`x-device-type`、`x-os-version`、`x-app-version`；请求体固定 `{"req_source":1}`。缺少设备头同样会被判成 9074。
- 业务码：`0` 成功，`9004`/`9095` 今日已签，`9074` 参与人数过多，`1001`/`1002` 鉴权失效；HTTP 429 与 5xx 与 9074 同等对待。
- 领取接口可能返回假成功：成功后用同一个设备号回查 `checked_in`，确认翻转才算签到成功；未确认不报成功，交给下一轮。
- 每轮最多发一次 claim，不连发、不换号重试；服务端未开放签到时不再发领取请求，避免白白消耗限流额度。
- 不刷新令牌。令牌失效返回 `authInvalid`，需要在官方客户端重新登录并保存。
- 网络请求跟随本机代理设置，与其它 TRAE 官方查询一致；上游脚本为多账号共享出口 IP 的场景强制直连，本工具按单机使用场景跟随系统网络。

## 积分统计与自动化

积分统计读取官方额度查询留下的快照，按区域、账号、时间与计费单位筛选。历史保留 365 天；观测消耗只计算已确认相同配额周期的相邻已用计数差值，跨周期、配额变更、币种变化或缺失字段不推算。没有可用来源的 Token 统计不展示入口。

平台设置提供每日签到、自动额度查询、低余额/到期/登录失效提醒与安全轮换。全部默认关闭，国内版与国际版独立保存。每日签到只对国内版生效，领取时间按本机时区（HH:mm）；同一天最多尝试 3 次、间隔 30 分钟，错过后在下次启动时补领；参与人数过多只算排队，不当作失败提醒。桌面端与 WebUI 服务各自注册后台循环，跨进程文件锁避免重复执行；客户端运行中、进程状态未知或额度不明时不切换，使用前置账号校验阻止覆盖用户刚刚完成的手动切换。

本地备份和 WebDAV 同步复用现有 WBS 密码派生及 AES-GCM 加密信封，跨设备恢复时重新使用目标系统凭据保护。TRAE 云端文件固定为独立的 `trae-accounts.wbs`，并沿用 ETag 并发写保护；同步合并缺失账号，不覆盖已保存凭据。密码错误、文件变化或非 TRAE 载荷在修改账号库前拒绝。任务日志保留最近 200 条，同类提醒每账号每天最多一次。
