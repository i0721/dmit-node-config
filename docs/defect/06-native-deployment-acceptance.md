# Rust 原生化与 DMIT 自动部署验收记录

日期：2026-10-17。实施范围与顺序见 [实施计划](04-native-deployment-plan.md)，安全发现见 [安全审查](05-native-deployment-security.md)。本报告区分本地验证与生产执行，不将功能实现等同于公网验收。

## 实施交付

| 要求 | 实现与证据 |
| --- | --- |
| 项目主动 Python 依赖移除 | 六个 Python 文件删除；仓库工具、TLS 探针、地址解析迁入 Rust；脚本、Actions、Docker、发行与安装检查全部改接 Rust；check-native 扫描通过 |
| DMIT 默认值完整继承 | Config、Schema 和既有 Nginx/Xray/systemd 模板不变；新增配置从完整 example 派生；测试验证权重、证书、路由等基线；显式输入覆盖、有效旧配置与身份保留 |
| 系统与交互配置 | deploy 同时支持交互默认值、显式参数和受保护完整输入；系统/IP/服务/文件/路由盘点；按需依赖准备与锁定下载；凭据无回显或 owner-only 文件 |
| DNS | 固定 Cloudflare API、限定 zone/名称、灰云、双栈、重复/CNAME/残留 AAAA 拒绝、元数据/TTL 保留、公开传播验证、请求意图日志及漂移保护恢复 |
| 双证书 | 复用有效证书；域名 DNS-01/P-384、IP HTTP-01/shortlived；原 SAN/信任链/密钥/有效期校验；私钥保留、冲突拒绝；续期 hook/cron 与旧任务退役 |
| WARP | 复用设备凭据或明确接受条款后注册；独立 nativetun；临时 SOCKS 公网探测后恢复原服务；默认路由前后比较 |
| 节点应用与升级 | 调用原 render/diff/check/takeover/apply/recover；已有 0.1 配置及快照验证；当前和兼容旧客户端三协议握手；部署阶段日志、受保护备份、失败材料保留与恢复 |
| 手动配置 | 原 setup/init/render/check/apply/upgrade 等保留；自动与手动共用配置模型、模板及服务事务，无第二套 Schema |

## 已执行的本地验证

环境：Windows 工作区、WSL Debian 的 Rust 1.99.1、Docker 隔离 Linux/systemd，x86_64。测试使用独立本地 CA/DNS 和真实 Nginx/Xray；不使用 Python 测试服务。

| 命令或检查 | 结果 | 本地日志（dist 为忽略目录） |
| --- | --- | --- |
| bash scripts/check-all.sh | 34 个主程序单测、2 个工具单测、4 个 CLI 测试通过；1 个上游测试默认忽略，另行执行如下；fmt/clippy/Schema/文档/工作流/Shell/native 检查通过 | native-quality-final.log |
| XRAY_TEST_BINARY=… cargo test --locked upstream_xray_accepts -- --ignored | 锁定官方 Xray 接受服务端及全部客户端配置，1/1 通过 | upstream-xray-final.log |
| cargo deny check | advisories/bans/licenses/sources 通过；已移除审计发现的停止维护依赖，无公告豁免；仍有重复依赖及未遇到许可的警告 | security-deny-final.log |
| bash scripts/release-package.sh x86_64-unknown-linux-gnu | 再次运行测试；生成 SBOM、许可集合、构建信息与归档；从精确归档提取程序通过 Schema 和兼容 smoke | release-package-final.log |
| 最终普通 release 程序 release-smoke | 通过 | release-smoke-final.log |
| Debian 13 完整集成 | 实际 systemd/TCP/UDP/UDS/H2、独立网络命名空间 H3、三协议、全部事务故障点、接管/升级/回滚/卸载恢复、自动部署门禁/失败/中断恢复通过 | integration-driver-final-debian13.log |
| Ubuntu 26.04 完整集成 | 同上，通过 | integration-driver-ubuntu26.log |
| Debian 12 手动完整集成 + 最终代码自动流程复验 | 手动生命周期通过；初次自动夹具被 hosts 抢答，修正 --no-hosts 后最终代码的部署、身份保留、升级、故障和中断恢复通过 | integration-driver-debian12.log；automation-final-debian12.log |
| Ubuntu 24.04 手动完整集成 + 最终代码自动流程复验 | 手动生命周期通过；同一 DNS 夹具修正后的最终代码自动部署、身份保留、升级、故障和中断恢复通过 | integration-driver-ubuntu24.log；automation-final-ubuntu24.log |

两项实现完成后开始集中验收。矩阵测试启动后补充的 ACME 缓存/共享账户保护由最终单测覆盖；最终 Rust 版本还在 Debian 12、Ubuntu 24.04 自动流程中重编译复验。手动证书夹具不验证公网 ACME 或 Cloudflare 写入。

本地归档 SHA256：`27817f1a818db51966ac5a16e3efd783b57679a8858766b449c960b0da80d6fb`。暂存到 VPS 的普通 release 候选 SHA256：`b86e3d84b0651c88c63f5ee59ed9bb0b6013502b37a4dc07b2f4c7570c2208d4`。二者编译目标形式不同，归档内程序另经独立 smoke，不混用其摘要。

## 真实环境当前证据

- SSH 经用户提供的自定义端口与既有 known_hosts 校验连接，未绕过主机密钥认证。原 nodectl 0.1.0，三个原服务正常，无未恢复部署日志。
- validate 接受原输入，deploy 验证旧快照并认定客户端兼容字段保留。操作者明确授权具体生产计划后，原 0.1 程序已单独备份，正式程序替换为已核对摘要的本地构建 0.2 候选；没有发布或冒充 GitHub 签名发行版。
- 操作者明确授权创建指定两个 zone 的 DNS Edit + Zone Read Token，有效期至 2027-10-17，已私密保存到目标 VPS。真实 Cloudflare API 只读预览成功，required_actions 为空。
- 已按原审批摘要执行六条灰云 A/AAAA 新增，公开解析通过；域名证书成功签发，SAN 覆盖新旧六域名，有效期至 2027-01-15。有效双 IP 证书原样保留，有效期至 2026-10-23。未重注册 CA/WARP、未更新现有 Nginx/Xray/usque/Go、未改 SSH 或防火墙。
- 部署内的 WARP 临时 SOCKS HTTPS 探针成功，原 TUN 服务恢复；完成收据确认 IPv4/IPv6 默认路由相同。当前与旧快照三协议握手通过，原服务与 cron 正常，doctor 全部通过，快照完整性有效，无待恢复部署/服务日志。
- 备份逐字节比较确认：原客户端 secrets、域名 TLS 私钥、IP 证书/私钥、WARP 配置及 Nginx/Xray/usque 程序完全相同。旧主域名 ACME conf 已退役为 removed，原材料保留，新缓存及 cron 启用。
- 域名和 IP 的实际 ACME 缓存均核实 reloadcmd 指向正式 `/usr/local/bin/nodectl` 的带锁 renew-hook；Token 文件 root:0600。测试用 Windows/WSL 客户端凭据副本及临时 Token 副本已删除，目标 VPS 的私密配置、Token 和恢复备份保留。
- 独立 Docker 客户端经过公网 IPv4 入口，新旧各三种协议全部成功，trace 出口为 DMIT 自有 IPv4/IPv6 且 warp=off；公网 H2=2、H3=3、IP TLS=200，证书验证开启。日志为 public-protocols-docker-final.log、public-web-final.log；主机细节与备份比较日志在私密 .local-deploy 目录。
- 本机 WSL 原生 curl 直连曾返回 TLS EOF；从独立 Docker 网络及本机代理路径访问成功。未把该路径失败伪报为成功，也未为通过测试降低证书/TLS 校验。独立客户端首个检查误假定只有 IPv4 出口，核实实际为原 DMIT IPv6 后，按两种既有地址严格核对并重跑六组通过。
- [教程截图](../cloudflare-screenshots.md) 来自真实控制台，区分未提交 DNS 草稿、实际创建 Token 和实际六条生产解析，不含凭据。

## 未覆盖及不可替代的验收

生产执行已取得明确授权并完成上述验证。自然续期周期尚未到达，不能把 renew-hook 配置、证书检查及 reload 成功视作周期验收。

本轮未执行 ARM64 实机矩阵、GitHub OIDC 签名发布/签名负例安装全流程、新空白 VPS 的整套 apt/Go/CA/WARP 首次注册、公网 DNS 补偿故障注入。IPv6 DNS、SAN、监听、主机本地 IPv6 IP TLS=200 及 DMIT IPv6 直出已经检查；本地客户端没有独立公网 IPv6 路径，未声称已从外网走 IPv6 入站。相关实现与门禁保留，但不声称这些外部场景已实际通过。没有原安装基线的 0.1 主机不支持猜测卸载；包数据库、新用户、CA 已签发资源仍有明确恢复边界。

结论：两项实现、本地回归/恢复审查以及经授权的原 DMIT 0.1 到 0.2 兼容升级已完成。现有生产服务、客户端身份及 DMIT 配置标准保留，上述未覆盖场景与恢复边界作为交付限制明确记录。
