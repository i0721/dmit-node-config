# 双域名、中性页面与 UDP/QUIC 修正实施记录

本页为历史功能验收摘要，环境和恢复标识已脱敏；原始日志与完整记录私密保存，不作为可直接执行的生产指令。公开截图均为脱敏示例。

本轮按已确认方案实施：主域名 speedtest，REALITY SNI files，两名称共同解析及签证书；一次切换 Encryption/Vision，保留 UUID、路径、端口、权重、IP 证书及默认路由。

顺序：先保留旧格式验证器，再增加可缺省配置/密钥、Rust 同源客户端导出及协议设置；随后补齐首页和证书材料备份、持久恢复、续期兼容；最后执行两内核三传输 TCP/UDP/强制 QUIC、负例、恢复和手动回归。通过隔离检查后提交新的具体生产预览并单独请求授权。

公开页面仅替换已确认的供应商占位内容或初始化缺失首页，不覆盖其他网站。旧 DNS 留作回滚。服务端域名证书仅覆盖新两个名称，恢复必须还原旧证书与首页，不能仅切换快照。旧格式按原序列化和模板严格验证，不放宽摘要。

目标客户端为锁定 Xray 26.10.10 和 Mihomo v1.19.32。XHTTP TLS ALPN 为 h2,http/1.1，WS 保持 http/1.1；服务端 Vision，Xray 客户端 Vision-udp443，Mihomo Vision+UDP+XUDP。全部三种入口使用分别生成的配套 ML-KEM-768 认证参数，不在预览/render 中生成或轮换。

所有验收结果与限制须据本轮真实日志补充；计划描述不代表已经上线。

## 实现

- 0.3 使用模板版本 2；`render_legacy.rs` 固定原版本 1 的生成逻辑，0.1/0.2 输入序列化继续省略新字段，仍核验 release ID、输入摘要、文件摘要、文件清单和完整重生成内容。未知版本拒绝加载。
- 新配置开启 VLESS Encryption；旧输入缺少该节不会在 init/preview 中被补成开启。三对 ML-KEM-768 参数由锁定 Xray 显式初始化一次，预览不制造密钥，重复初始化不轮换。原 UUID、REALITY 密钥与 short ID 独立保留。
- Rust 同源生成 Xray 和 Mihomo 节点；`export-clients --format all` 输出 `profiles.json`、`proxies.yaml` 和 `weights.json`。Mihomo 文件为安全序列化的 YAML 1.2 JSON 子集，仅包含节点，不改客户端规则。导出仍要求成功应用的版本。
- `--replace-domain-set --vless-encryption true` 明确执行不兼容迁移，清除未显式指定的原别名。旧 DNS 不删除。新默认两个域名、各自 A/AAAA、域名证书两个 SAN；独立 IP 证书保持不变。
- 缺失或精确匹配已知占位页时创建 Welcome 页面；自定义内容、普通符号链接及悬空链接均不覆盖。标题、主标题与正文不含供应商信息。
- 自动部署备份加入首页，完成记录绑定本次 release 并保存材料和受管入口指纹；rollback/recover 校验原始备份后恢复输入、证书、首页、续期材料、服务和 DNS。准备阶段日志使用格式 2，旧格式 1 严格按原备份范围恢复，不要求其备份从未修改的首页。新增受保护 `--token-file`。材料漂移、备份损坏或过期证书会阻止恢复，不降低摘要、证书或 DNS 并发检查。
- 续期只刷新与当前证书兼容的历史检查收据。已退役域名集合的收据保留原值，不能静默改绑新证书。

## 已确认的测试证据

日期：2026-10-18（Asia/Shanghai）。原始日志在忽略目录 `dist/`，不随公开仓库发布凭据。

| 检查 | 结果与证据 |
| --- | --- |
| Rust 本地质量门禁 | 40 个主程序测试、2 个辅助测试、4 个 CLI 测试通过；另有 2 个需真实内核的测试单独执行。fmt、Clippy 全目标全特性、Schema 对比、文档/工作流/原生依赖门禁通过。`neutral-check-all.log` |
| 官方内核配置与初始化 | Xray 26.10.10 与 Mihomo 1.19.32 的实际解析检查通过，覆盖旧配置及加密服务端/客户端；真实参数生成后重复初始化不变。`neutral-upstream.log` |
| 依赖审查 | cargo-deny advisories/bans/licenses/sources 通过；保留既有重复依赖及未匹配许可声明提示。`neutral-deny.log` |
| 本地发行包 | amd64 构建、SBOM/许可证收集、归档内二进制 Schema 与 smoke 检查通过；未发布或签名。`neutral-release-package.log` |
| DNS 补偿逻辑 | 测试传输覆盖新增删除、原记录及元数据恢复、未收到响应的 pending 恢复、重复恢复、并发漂移拒绝；不把它表述为真实 Cloudflare 故障演练。 |
| ACME 准备阶段 | 隔离容器中使用已有本地 CA 的真实叶证书和模拟 ACME 命令，走生产签发/安装调用路径，确认复用 P-384 私钥、安装阶段显式延迟 reload、加密配置不依赖虚假密钥生成操作参数。`neutral-acme-cli.log`、`neutral-acme-ubuntu24.log` |

跨系统证据按阶段记录；只有日志中的实际完成标记表示通过：

| amd64 系统 | 两内核三传输 TCP/UDP/QUIC/无泄漏、24 组负例 | 手动服务事务与恢复 | 新材料迁移/回滚与 ACME 调用 |
| --- | --- | --- | --- |
| Debian 13 | 通过，含真实 ClientHello | 10 个故障点、恢复重试及卸载恢复通过 | 故障恢复、完成后完整回滚、入口漂移拒绝、续期 hook 通过；另有新空白隔离容器复验及 ACME 准备调用通过 |
| Debian 12 | 通过 | 故障点及卸载恢复通过 | 恢复 WSL 后自动迁移、材料失败恢复、完整回滚、续期 hook 与 ACME 调用通过（`neutral-final-auto-debian-12.log`） |
| Ubuntu 24.04 | 通过 | 故障点及卸载恢复通过 | 完整材料回滚/续期 hook/漂移保护通过，ACME 调用补测通过 |
| Ubuntu 26.04 | 通过 | 恢复环境后补齐 nginx/services/health，全部故障点、恢复重试和卸载恢复通过 | 自动迁移、完整材料回滚、漂移保护、续期 hook 及 ACME 调用通过（`neutral-final-ubuntu26.log`） |

网络/手动日志为 `integration-<system>-x86_64.log`；最终材料日志为 `neutral-final-materials.log`、`neutral-final-auto-<system>.log`。早期整体驱动在测试夹具将 JSON 键重新排序后，用字节摘要误报身份变化；已改为首次部署比较规范化 JSON 摘要，之后的故障恢复和完整回滚仍逐字节比较原文件。修正后单独续跑自动阶段；ACME 夹具也补齐既有账户标记并独立复测，保留原始失败日志，不隐藏早期退出状态。

## 本地环境恢复与最终构建

2026-10-18 本地 WSL 曾阻塞接口查询，内核日志有 hv_netvsc 子通道内存分配失败信息；未据此断言根因。操作者重启 WSL 服务后环境恢复，保留测试容器并按原地址顺序启动，继续未完成阶段。未修改 WSL 网络设置或 Windows 网络配置。

最终 amd64 候选 SHA256 为 `PRIVATE-ARCHIVE`。已重新上传并只读校验生产配置和 0.1 快照。四个 amd64 系统隔离阶段全部通过后，按该二进制及新计划摘要取得独立授权；生产执行与最终证据见下节。

## 互通问题及修正依据

Mihomo 默认移除 REALITY 指纹中的 X25519MLKEM768，而锁定 Xray 要求该 key_share。首次真实测试中 Xray 三传输及 Mihomo TLS/WS 成功，Mihomo REALITY 出现认证失败。依据[锁定 Mihomo 源码](https://github.com/MetaCubeX/mihomo/blob/v1.19.32/component/tls/reality.go)增加其支持的 `reality-opts.support-x25519mlkem768: true`，未添加无效 Xray 字段或跳过认证。修正后两内核三传输的 TCP、UDP DNS、多包回显与强制 QUIC 已实测成功，最终生命周期结果继续记录。

透明 TCP 中继只记录 ClientHello 的公开 ALPN/key_share 元数据，字节原样转发，不终止 TLS、不保存载荷或秘密。观察到 XHTTP TLS 为 h2,http/1.1，WS 为 http/1.1，REALITY 含混合密钥交换且排序先于 X25519。协议测试使用独立客户端 TUN/网络命名空间，停止代理后同一强制 HTTP/3 请求须失败。

## 安全审查与生产边界

密钥、API Token 和客户端导出保持私密权限；普通差异与错误不输出工具原始密钥内容。三组私有 decryption 参数不进入客户端导出。正式构建排除测试 CA 获取、故障注入及测试私网放行逻辑，已检查正式二进制。恢复记录受保护且校验完整文件清单、备份所有权/模式/内容、release 绑定和 DNS 并发状态。

生产执行前提交具体预览，明确四条 DNS、两个 SAN、旧客户端必须更新、维护中断和完整恢复步骤；操作者另行授权本次 0.3 切换。此前 0.2 的授权没有用于本次切换。

生产只读盘点发现主域名已有 A 记录，因此实际是三条新增、一条更新，而非四条新增。操作者已明确授权替换这条 A 并关闭代理；它已并入整次切换事务并保留原记录供恢复。没有在完整计划获授权前单独修改 DNS。

ARM64 实机、GitHub OIDC 签名发布/签名负例安装、自然续期周期、真实 Cloudflare 补偿故障和新空白 VPS 注册未在本轮重新实测。不得用本地配置检查或模拟传输冒充这些外部验收。已签发 CA 资源不能由本地回滚撤销；旧 DNS 清理属于上线验收后的独立预览。

## 经授权的生产执行与验收

2026-10-18（Asia/Shanghai），操作者明确授权计划 `PRIVATE-ARCHIVE`。执行前再次核对摘要一致、原 0.2 程序及 current 未变、无未恢复事务，备份旧程序后原子安装上述最终候选到 `/usr/local/bin/nodectl`，从正式路径完成部署。

新 release 为 `PRIVATE-ARCHIVE`，previous 为 `PRIVATE-ARCHIVE`。材料备份位于 `/var/lib/dmit-node/deployment-backups/PRIVATE-BACKUP`；受保护完成记录保存 19 项材料/入口指纹及四条 DNS 操作。备份旧程序和执行日志位于 `/root/nodectl-review-20261018/approved-03`。无需触发失败恢复；完整恢复能力由隔离故障测试验证，没有在生产人为制造中断。

| 生产验收 | 实际结果与证据 |
| --- | --- |
| DNS、证书与中性页面 | speedtest/files 各 A/AAAA，全部仅 DNS；三新增一更新。两域名 SAN 精确匹配，TLS 私钥复用；Welcome 标题/主标题及指定正文已通过浏览器和双栈 HTTPS 检查。`neutral-production-renewal-page.log` 及下方截图 |
| Xray 26.10.10 | XHTTP TLS、WS、REALITY 全部通过 TCP HTTPS、四轮 UDP DNS、四轮普通 UDP STUN、强制 HTTP/3 UDP443；停止代理后同一 QUIC 请求均失败。`neutral-production-protocol.log` |
| Mihomo v1.19.32 | 相同三传输及全部 TCP/UDP/QUIC/无直连泄漏检查通过；实际使用 UDP/XUDP/Vision 和原生 REALITY 混合交换开关。公网普通 UDP 使用 STUN，变长多包回显另在四系统隔离环境通过。 |
| 拒绝错误客户端 | 两内核 × 三传输 × 错误密钥/错误 flow/缺失加密/旧客户端，共 24 组，全部拒绝。上述日志末尾 `PRODUCTION_TWO_ENGINES_THREE_TRANSPORTS_ACCEPTANCE_PASS`；未用 TCP 回退冒充 QUIC。 |
| 真实 ClientHello | 两内核 XHTTP TLS 为 h2,http/1.1，WS 为 http/1.1；REALITY 观察到 h2,http/1.1 及先于 X25519 的混合 key_share。只记录公开握手元数据，不保存负载。 |
| 配置、密钥与出口保留 | 去掉新增 vless 字段后原 secrets 规范化内容一致；域名私钥、IP 双栈证书/私钥、WARP 配置及三个运行程序逐字节一致。默认 IPv4/IPv6 路由一致，实际双栈 trace 地址为原 VPS 地址且 warp=off。`neutral-production-preservation.log`、`neutral-production-runtime-preservation.log` |
| 续期兼容 | 正式 renew-hook 成功检查并 reload；域名与 IP 的 ACME reloadcmd 均解码核对为正式程序路径，旧六 SAN 收据摘要不变，旧主域名 conf 已 removed，cron active。尚未经历自然续期周期。 |
| H2/H3 与 IP TLS | 外部 Docker HTTPS 取得 H2=2；WSL 主机网络中的 curl 容器强制 H3=3。VPS 本地两域名及两个 IP 均以真实强制 H3 取得 3，证书验证开启。普通 Docker NAT 路径首次 H3 超时，保留原日志；没有把该次超时计为通过。`neutral-production-web.log`、`neutral-production-h3-host.log`、`neutral-production-h3-vps.log` |
| 运行状态与权限 | doctor 全项通过，nginx/xray/usque-warp/cron active，无待恢复部署/服务日志；Token、secrets 和客户端文件为 root:0600，导出目录 0700。`neutral-production-doctor.log` |

新客户端已通过 `export-clients --format all` 导出为私密 `profiles.json`、`proxies.yaml`、`weights.json`。Mihomo 文件仅包含节点，应合并到现有配置，保留自定义规则。Windows 交付副本位于忽略目录 `.local-deploy/neutral/client-bundle`，限制为当前用户和 SYSTEM；凭据不写入公开报告。旧客户端需要更新，不宣称未知版本手机客户端已验收。

恢复时使用正式 0.3 的 `nodectl rollback --token-file /root/nodectl-review-20261017/cloudflare-token`；中断事务使用同一路径的 `recover --token-file`。先核对材料、旧 current、DNS、证书和服务恢复成功，再将 `approved-03/nodectl-before` 还原为正式程序。不能先降级程序后忽略新恢复记录。现场漂移和备份完整性检查不可绕过。

旧 DNS 暂留供回滚，未擅自删除；只读核对了六个旧名称的 12 条 A/AAAA，独立清理预览保存在私密 `.local-deploy/neutral/retired-dns-cleanup-preview.md`，执行需要另行授权。外部 IPv6 入站路径、ARM64 实机、自然续期周期、签名发布、真实 Cloudflare 故障补偿及全新 VPS 首次注册仍是上述实测边界。

公开字段示例见 [Cloudflare 部署图解](../cloudflare-screenshots.md)。
交付清理：七个本轮隔离测试容器已删除，WSL 临时生产客户端副本已移除；保留构建缓存、验收日志、VPS 恢复材料及 Windows 私密客户端交付文件。清理后的再次检查确认生产 current 不变、四个服务 active、无待恢复日志。
