# 豆包桌面端接入

豆包是第五个平台，账号库与 WorkBuddy / CodeBuddy、Qoder、TRAE、MiniMax Code 隔离。支持账号识别、加密保存、重命名、删除、关闭并切换、官方额度查询、每日签到、额度历史、自动查询/签到/提醒/轮换、加密导入导出和独立 WebDAV 同步。桌面客户端没有对应的原生 CLI；`wb-switch` 提供 `--target doubao-desktop` 管理入口。

## 参考实现与本机核对

- [smart-open/TraeWorkAssistant](https://github.com/smart-open/TraeWorkAssistant/tree/b075a819d4b812895a1868b77e6972529a9ed822)（MIT）：核对多 Profile 登录态快照、`Local State` 的账号与会话、额度 `quota/summary/`、每日签到 `detail`/`landing/checkin` 的业务语义。只参考行为和字段，没有复制实现。
- [L0NE-6/DoubaoWork-Daily](https://github.com/L0NE-6/DoubaoWork-Daily/tree/fc6db08100f9edff821a881c0b7ea24a1e33db6d)（MIT，v1.3.2）：同一协议族的每日签到实现，用于核对 `campaign/v3/detail` 的 `daily_signin` 判据、`landing/checkin` 回执与回查。注意它**没有**独立的逐项领奖流程：`/alice/ug/campaign/item/reward/claim/` 只是旧端点名，仅用于旧台账迁移，实际提交只有一条 `landing/checkin {"scene": group}`；`campaign/list/` 是非 v3 活动组时的详情接口。`landing/report` 这类合成写请求上游已移除，我们同样不发。
- [cyz824/DoubaoSwitcher](https://github.com/cyz824/DoubaoSwitcher)：核对独立豆包切换工具的账号场景；功能范围较小，不作为登录态协议依据。
- 本机 Windows x64 豆包安装路径为 `D:\Program Files\Doubao\Doubao.exe`。当前用户的 `Local State` 活跃 Profile、Profile 身份、`Preferences` 登录资料与 `x-tt-multi-sids` 会话对应同一账号。对官方额度和签到状态端点只读请求均返回 HTTP 200 / 业务码 0；当前账号订阅额度窗口为空、每日签到任务存在。测试不输出账号 ID 或令牌。

## 使用

在豆包登录账号后点「识别本机账号」。工具会请求豆包正常退出，完整读取当前和其他 Profile 的登录态，加密保存，然后重新打开。切换时先将现场登录备份并收入库，再恢复目标登录态、读回验证身份，最后重新打开；客户端拒绝退出时停止。失败时使用加密恢复记录恢复原登录。保留项目、下载与对话历史。客户端仍在运行时不直接修改 Chromium 登录数据库。

保存成功后会立即查询一次官方额度，不必再手动点「刷新额度」。客户端重新登录或重启会轮换 `x-tt-multi-sids`，此时已保存的会话失效但账号仍在本机登录，工具会提示「重新识别本机账号」而不是要求重新登录。只有官方业务码 `710012001` 才会把账号标记为登录态失效；网关或代理返回的裸 HTTP 401 只提示稍后重试，不会禁用切换。关闭豆包时，有可见窗口的进程走 `WM_CLOSE`，仅托盘/后台的无窗口进程补发不带 `/F` 的 `taskkill`，未保存内容始终留给用户在官方客户端确认。

签到链路按官方前端走：先 `POST /alice/user/launch` 取 `ug_campaign_info.campaign_group_code`，再按组选详情接口（v3 组用 `/alice/ug/campaign/v3/detail/`，其它组用 `/alice/ug/campaign/list/`），只在 `daily_signin` 未签时提交一次 `/alice/ug/campaign/landing/checkin {"scene": group}`。回执除顶层 `code` 外还校验 `data.biz_error.code`；提交后按 0 / 1.5 / 3 秒回查三次，把官方落库延迟和「其实已领到」区分开，回执不明时只报待确认、绝不重复提交。

额度来自官方 `/alice/commerce/sale/subscription/quota/summary/`，按每个窗口展示已用和剩余百分比、重置时间、订阅状态。百分比不能跨窗口或账号相加，无法换算成积分或 Token；官方没有返回窗口时显示“未知”。签到先查询 `/alice/ug/campaign/v3/detail/` 的 `daily_signin` 状态；仅未签到时提交一次 `/alice/ug/campaign/landing/checkin`，随后回查，不明回执不重提。

`~/.wb-switch/doubao/accounts.json` 只保存元数据和系统加密的授权；本机完整登录快照在 `profiles/*.enc`。Windows 使用 DPAPI，macOS 使用 Keychain 派生密钥与 AES-GCM。密码加密的便携备份和 WebDAV `doubao-accounts.wbs` 只迁移账号授权，不迁移与设备密钥绑定的桌面登录快照；导入账号可查额度和签到，在新设备登录并识别一次后才可切换。自动轮换默认关闭，只在客户端完全退出、当前与候选额度均新鲜且可比时执行。

## CLI

```sh
wb-switch accounts --target doubao-desktop
wb-switch status --target doubao-desktop
wb-switch inspect --target doubao-desktop
wb-switch credits --target doubao-desktop
wb-switch rewards --target doubao-desktop
wb-switch switch doubao-<账号ID> --target doubao-desktop --dry-run
wb-switch switch doubao-<账号ID> --target doubao-desktop --close-running --relaunch
wb-switch login --target doubao-desktop
wb-switch credit-stats --target doubao-desktop --refresh
wb-switch automation get --target doubao-desktop
wb-switch call get_doubao_status
wb-switch call doubao_action --input-file body.json
```

`login` 打开官方客户端，待用户登录并退出后保存账号。`doubao_action` 使用 `{"action":"capture","input":{"closeRunning":false}}` 等请求；动作还包括 `switch`、`credits`、`checkin`、`checkin_status`、`statistics`、`get_settings`、`save_settings`、`automation_run`、备份与云同步。`token_stats` 明确返回不可用，避免把额度百分比伪装成 Token 用量。

## 验证边界

已用本机数据核对账号识别与官方只读接口；本机账号加密入库、额度查询、签到状态查询、切换预演及同账号切换后身份核验均通过，恢复记录清理且加密备份存在。自动测试覆盖双账号切换后身份核验、来源账号保留、快照路径拒绝、LevelDB 完整性与未知额度处理。真实桌面切换、首次签到发奖、macOS arm64 与真实 WebDAV 服务仍需对应环境验收；本次不自动领取当前账号的奖励。
