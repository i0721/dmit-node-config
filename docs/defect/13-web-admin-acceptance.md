# Web 管理后台实施与验收记录

2026-10-19。本机隔离实现与验收已完成；本文件记录隔离环境证据，不代表已部署生产或发布 v0.6.0。

## 实施范围

- `webui` 来源为 rin721/console-web 固定提交 `60189fd7ccbc1c034034ed0ca5ae30d61a448026`，没有嵌套 `.git`。复用原设计 Token、UI Adapter、Page/Section Pattern、Plugin Registry 和路由生成器；生产只装配 DMIT 七页。
- Rust 单管理员 Token 与订阅 Token 独立；摘要保存、首次显示、CLI 重置、Secure/HttpOnly/Strict Cookie、空闲与绝对期限、Origin/CSRF、登录和并发限流、敏感日志保护。
- 默认只允许环境资产白名单。平台修改要求重新认证、单次高级解锁、原值与渲染差异、真实检查、计划标识二次确认。Rust 绑定会话、Token generation、配置/密钥字节摘要、current 和期限，拒绝未知字段与不可变边界绕过。
- 变更复用 validate/render/diff/check/apply、原 deploy/upgrade、快照、输入 journal、失败恢复和回滚。快照恢复重新认证并保留管理员/订阅 Token；新增身份恢复计划不会混用保留现有密钥的完整部署路径。
- 七页全部使用实际 API：procfs/statvfs、systemd、双栈接口/路由、DNS、双证书 SAN/日期、成功发布客户端、订阅轮换/撤销、任务与审计。
- 独立 Nginx master 与 API systemd 单元，默认 HTTPS 9443，原 443、REALITY 与节点 Nginx 不参与后台安装。静态 CSP 采用实际构建脚本摘要，生产不运行 Node.js。
- 发行归档携带静态 UI、版本/摘要清单与前端第三方声明；`install.sh --with-admin` 复用原 OIDC/Sigstore 验签链路。Rust 拒绝版本不符、越界路径、摘要损坏、私密文件和过大输入。
- API 重启先建立写入屏障，再由受控 systemd 定时器执行；旧会话失效。CLI 原子替换后，未重启 API 的子操作仍绑定运行中的可信映像。

## 已验证的核心验收

| 要求 | 证据与结果 |
| --- | --- |
| DMIT 默认配置不被覆盖 | `git diff --exit-code -- config templates schemas src/render.rs src/config.rs` 通过；真实后台安装与端口冲突重装失败后，节点配置/密钥/Nginx/Xray 摘要及原服务 PID 均保留。 |
| 后端强制安全模式 | 真实进程 HTTP 测试与 HTTPS 浏览器请求验证保护字段 403、未检查 apply 409、二次确认、单次解锁、计划归属/失效和硬约束；失败前后原输入不变。 |
| 高级修改可追踪、可恢复 | 实际 Linux Nginx/Xray 完成高级预览、真实检查、应用、快照恢复及两次回滚；最终配置和密钥摘要恢复，审计记录可读取。另有输入 journal 精确恢复与损坏快照拒绝测试。 |
| 认证与敏感信息保护 | 独立 Token、Cookie 安全属性、Origin/CSRF、超时、CLI 重置撤销会话、限流、导出与日志脱敏均有测试；节点事务持锁时仍可撤销管理员身份。 |
| 业务界面与 API 联调 | 七页真实 HTTPS、客户端实际下载、界面高级编辑/检查/确认/应用和快照恢复已通过；桌面/手机、深色、WCAG AA、无横向溢出、无浏览器持久化凭据。 |
| 静态发行安全 | 实际归档安装与失败重装恢复通过。构建后再运行 typecheck 不再污染静态目录；生成类型位于 `.next/typegen`，安装器继续拒绝 `.ts`。 |
| 原 CLI/自动部署兼容 | 更新后 v0.6.0 原全流程已得到 INTEGRATION_PASS，覆盖实际协议/UDP/QUIC、故障注入、恢复、配置升级、回滚、卸载恢复、证书/域名回滚和向导。 |

## 测试账本

- Rust 全门禁：69 个主程序测试、3 个 repo-tools 测试、1 个真实 HTTP 测试、5 个原 CLI 测试通过；fmt、clippy all-targets/all-features、Schema、文档、工作流固定版本和 native audit 通过。
- 既有显式上游测试：官方锁定 Xray/Mihomo 的两项服务器/客户端配置校验通过；独立 mount/network 命名空间 CN 双栈策略与两项内核恢复测试通过。需要真实工具的忽略测试按各自隔离驱动执行，不能将普通 cargo test 的 ignored 数量当成通过数量。
- 前端：251 项单元测试、Foundation/Architecture/Dependency、两类 Codegen freshness、Lint、Type、静态构建和性能预算通过。新增浏览器测试另有单独 lint/type 验证。
- 最终静态预算：initial JS gzip 389044 bytes，最大路由 436176 bytes，CSS gzip 47452 bytes；预算未放宽。
- 浏览器：完整五项与新增 CLI 卸载中断 Web 恢复测试全部通过（六项业务测试），包含七页/移动/深色、下载、实际高级 API 工作流（含完整部署预览）、UI 写入恢复、CLI 原子替换后执行健康检查及两次实际 API 重启。登录/总览/深色/移动 WCAG AA 额外复核通过。
- 静态安装：`ADMIN_STATIC_ARCHIVE_INSTALL_AND_FAILED_REINSTALL_RESTORE_PASS`。
- Linux 原 CLI 全流程 INTEGRATION_PASS；LibreSpeed/OpenList 原生部署、维护隔离、中断恢复、提交后数据保留通过；官方原生与托管 HTTPS 的 64 MiB 上传/下载、Range、匿名/只读/沙箱验证通过；CN geosite/geoip 双栈 TCP/UDP、DNS 失败阻断、接口删除和无原生出口泄漏通过。

可分享的 [测试摘要](../../webui/docs/changes/112-dmit-console/evidence/verification.txt) 汇总命令、结果与限制。原始完整运行日志保存在本地忽略目录 `dist/admin-*.log` 与 `dist/integration-debian-12-x86_64.log`；完整五项浏览器 JSON 在 `dist/admin-final-browser-full.json`，卸载中断恢复 JSON 在 `dist/admin-uninstall-api-recovery.json`；额外可访问性日志为 `dist/admin-final-accessibility.log`。可分享截图位于 [112 证据目录](../../webui/docs/changes/112-dmit-console/evidence)。可复现后台入口为 `scripts/test-admin.sh`，原 CLI 入口为 `scripts/test-integration.sh`。

## 边界与遗留验证

- 未修改生产 VPS、生产 DNS、生产凭据或 `.local-deploy`。本记录汇总本地隔离验收；正式发布状态以 [v0.6.0 Release](https://github.com/i0721/dmit-node-config/releases/tag/v0.6.0) 为准。
- 未使用真实 Cloudflare/WARP 账户做外部写入或设备注册；其 API 协调复用现有 DNS/deploy/usque 机制，外部账号环境仍需运营方在实际部署中验证。
- 额外全新容器的初次安装、可配置端口 11443、原节点字节/PID 保留及失败重装恢复亦通过。当前机器执行 Linux amd64 隔离验收；其它发行版/arm64 继续由已有 CI 矩阵验证，未把本机结果扩展为所有平台的实测证明。
- 新增 CI 后台验收工作流与静态发行步骤已写入。远程构建和 v0.6.0 OIDC 签名发行证据由 GitHub Actions 发布工作流产生；本地测试归档仅用于隔离验收。
