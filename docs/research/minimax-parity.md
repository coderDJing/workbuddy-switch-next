# MiniMax Code 功能补齐记录

日期：2026-11-11。Windows x64；macOS arm64 与国际版真实登录/领取仍待实机验证。

## 参考实现

- [jlcodes99/cockpit-tools](https://github.com/jlcodes99/cockpit-tools/tree/7ca7f0db1f915452681c9bbc366f199e2310761f)：核对 MiniMax 账号切换、代际同步、签到与自动调度。该仓库默认 CC BY-NC-SA 4.0；本次只参考协议和行为，未复制其实现。
- [JeffTsuiCUG/dsh-connect-minimaxcode](https://github.com/JeffTsuiCUG/dsh-connect-minimaxcode/tree/f84bb466a3e6df4eb0ed4d7b18236a1076b0a000)（MIT）：核对 signin.ts 的 timezone_id、每日状态 1/2/3/4、claim_result 1/2 以及协议测试。
- [VICqiufeeng/dsh-minimax-connect](https://github.com/VICqiufeeng/dsh-minimax-connect)：原接入的凭据、额度与刷新共存参考。
- [MiniMax-AI/minimax-code](https://github.com/MiniMax-AI/minimax-code/tree/3c21163c85a9684030cb147bfef48c6284fa3c46)（MIT）：核对 oauth-core 代际和命名空间，以及 local-runtime-v2 的 canonical history 和 Token 桶定义。

## 已验证

- 45 项 MiniMax core 测试：领取只提交一次、状态回查、网络回执丢失、重复领取跳过、账号切换与 CLI 运行拒绝、自动轮换前当前登录校验、加密备份密码与快照校验、目标机凭据重建、云端授权冲突、统计去重和隐私边界。
- 3 项隔离家目录 CLI 集成测试：能力发现、区域设置、领取参数、真实路由和共享会话 Token 读取。
- 相关前端 12 项测试、生产前端构建与 Windows 桌面端 cargo check 通过。
- 浏览器逐页检查账号管理、积分统计、Token 统计、平台设置，含演示数据与全部新增入口。
- 本机真实 Token 统计：2 个会话，156 次请求，23,137,377 Token（含缓存），读取完整；返回不含对话内容。
- 本机国内版 rewards：官方返回 today already，CLI state=already、credits=0、退出码 0；未提交重复领取。余额刷新正常。

## 验证边界

真实首次发放新奖励、国际版、macOS arm64、真实 WebDAV 服务尚未验收。云端使用已有 WBS 加密与 ETag 传输层，新增平台合并逻辑有离线测试。Token 不具备可靠账号/区域归因；当前实现按日期、模型展示。本期未增加关联会话或悬浮窗生命周期事件适配。
