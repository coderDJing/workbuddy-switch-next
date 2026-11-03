# Qoder 首次登录前添加账号：Windows 隔离试验

日期：2026-11-02。此记录只描述本机 Windows 试验，不代表 macOS 或 Linux 的结果。

## 试验范围

- 以本机 Qoder CN 可执行文件的 Electron 版本 43.1.1 为基准，下载同版本官方 Electron 运行时，并校验发布包 SHA-256。
- 使用全新独立的临时用户目录和虚构账号 / 令牌；未启动 Qoder 客户端，也未向 Qoder 服务提交登录。
- 只读检查本机现有 Qoder CN 登录文件的加密形式和固定字段是否存在；不记录字段值、账号信息或凭据。

## 实测结果

1. 在空目录启动测试 Electron，调用 `safeStorage.isEncryptionAvailable()` 后退出，即使未登录任何账号也生成了包含 DPAPI 保护密钥的 `Local State`。
2. 在该空目录用 Electron `safeStorage.encryptString` 加密虚构数据，生成 `v10` 密文；重启测试进程后可解密。使用 Windows DPAPI 解开该目录的密钥，再用 AES-256-GCM 可解开 Electron 生成的密文；反向生成同格式密文也能被 Electron 解开。
3. 不使用 Qoder 的数据目录，单独通过当前 Windows 用户的 DPAPI 加密虚构账号数据；另一个进程可成功解密。这证明本工具可以拥有独立账号库，无需先生成 Qoder 客户端登录文件。
4. 本机 Qoder CN 的现有桌面登录文件可按 `v10` / DPAPI / AES-256-GCM 解密。只检查字段存在性，发现除令牌、到期时间和用户信息外，还有 `schemaVersion` 和 `authenticationContext` 等字段。

## 结论与边界

- **已证实**：Windows 上，独立账号加密存储和 Electron 加密材料初始化都不依赖已有 Qoder 登录。
- **尚未证实**：全新 Qoder 桌面客户端是否在未登录时创建相同的密钥；手工构造的完整登录文件是否会被真实 Qoder 接受；实际设备授权、令牌刷新和账号切换是否成功。独立 Electron 测试不能代替这些验证。
- **尚未验证**：Qoder 国际版、macOS 钥匙串、Linux 密钥环及其首次启动行为。
- 后续应先实现独立账号库，再在客户端已初始化的隔离环境中验证桌面端材料生成、身份与令牌刷新。无法确认完整格式时应拒绝写入，而不是覆盖用户的现有登录。

客户端凭据格式及区域差异可参考 [Buddy Switch](https://github.com/NextAgentX/qoder-trae-workbuddy-switch) 和 [qoder-switch](https://github.com/JIUSHIQINGSHAN/qoder-switch)；本次实测以本机 Windows 客户端为准。

## 后续核对：网页登录和切换的未决问题

- 两个公开项目对桌面版授权入口有不同说法：[Buddy Switch 的区域参数](https://github.com/NextAgentX/qoder-trae-workbuddy-switch/blob/main/crates/buddy-switch-core/src/modules/qoder/region.rs)使用旧入口；[cpa-multi-plugins 的协议说明](https://github.com/mmqz/cpa-multi-plugins/blob/main/docs/PROTOCOL.md)描述了另一组桌面版参数。对本机 CN 0.4.8 桌面程序的混淆字符串表进行静态解码，发现新入口域名和客户端 ID，而旧客户端 ID 不在该字符串表；已按新桌面版协议构造两区域授权 URL。实际授权仍需验证。
- [Buddy Switch 的桌面切换实现说明](https://github.com/NextAgentX/qoder-trae-workbuddy-switch/blob/main/crates/buddy-switch-core/src/modules/qoder/switch.rs)要求保留既有登录文件中账号库未覆盖的字段，并指出客户端退出时可能写回内存中的登录态。原型之前在关闭客户端前生成目标快照和保存当前登录；现已改为退出后在同一切换锁内处理，并增加关闭时改写凭据的模拟测试。
- 独立账号库与客户端可能持有同一刷新令牌。刷新可能发生轮换；必须确认服务端行为并设计令牌所有权，避免一个客户端刷新后另一个客户端或库中凭据失效。不能仅凭加密往返测试声称账号能长期正常切换。

## 本机 CLI 格式核对

- 已只读检查本机 Qoder CN 0.4.8 所附的 `qoder-cn-agent-sdk` 实现与本机 CLI 凭据的字段名和类型，不记录字段值。其当前凭据使用 `uid`、`security_oauth_token`、`access_token`、`refresh_token`、`expire_time`、`refresh_token_expire_time` 等字段；启动流程会重新生成 `encrypt_user_info` 和 `key`。
- 原型已经按该结构生成 CLI 登录数据，清除上一账号的身份与启动验证缓存，并处理授权响应中的绝对 / 相对过期时间。切换准备已移到客户端退出后、同一切换锁内；模拟退出时更新凭据的测试已覆盖。
- 对本机桌面程序的凭据读取器做静态解码，提取到其字段检查表达式；用独立 JavaScript 环境验证合成文件可通过该表达式，损坏文件被拒绝。Windows 原型已支持为全新桌面目录生成加密密钥与符合字段检查的登录文件，并在两区域模拟测试中通过。
- 这仍是静态代码和模拟存储验证；尚未用真实账号确认官方客户端接受生成的数据，国际版桌面程序也未在本机实测。跨客户端自动续期仍未解决。

## 国内版真实网页登录验证

- 用户在本机开发版通过 Qoder 官方网页完成国内版授权并添加账号。只核对本机账号库的脱敏状态：桌面与 CLI 两份独立加密凭据属于同一身份；原有客户端账号已分别保存，且与新授权账号不同。
- 读取该授权的 DPAPI 密文，在独立临时目录中生成国内版桌面和 CLI 凭据，并验证两端的身份和令牌与授权结果一致；测试结束后删除临时目录。测试没有启动 Qoder 或改写真实客户端数据。
- 在已有登录文件的本机 Windows Qoder CN 0.4.11 上，实际切换至新账号并启动客户端，界面显示新账号；随后退出客户端、切回原桌面账号，并核对原桌面与 CLI 登录文件哈希均已恢复 / 未改变。
- 使用官方 `qoderclicn --config-dir <隔离目录> status --output json` 读取生成的 CLI 登录状态，确认其返回的是新授权身份；不改变用户当前 CLI 的登录目录。未发起模型对话。
- 全新桌面目录的真实客户端首次启动、国际版、自动续期和跨客户端同时使用尚未完成真实验证。
