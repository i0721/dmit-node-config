# CN WARP 与网站服务阶段验收记录

日期：2026-10-18。状态：**本地实施及下述隔离验收通过，v0.5.0 已正式发行；未修改生产 VPS，不是生产验收通过报告。**

## 已取得的证据

| 项目 | 实际结果 | 本地证据 |
| --- | --- | --- |
| 默认测试与门禁 | 当前候选重跑：主程序 52、仓库工具 2、CLI 5 通过；fmt、Clippy 全目标全特性、Schema、文档、工作流和 Python 零依赖检查通过，新增集成脚本语法检查通过 | `dist/cnweb-check-all.log` |
| 0.4 快照 | 原正式签名 0.4 二进制生成的真实模板 3 快照，由 0.5 开发程序完整验证 | `dist/cnweb-original-040-snapshot.log` |
| 策略所有权 | IPv4/IPv6 mark、拒绝路由、接口删除、原生默认路由与他人资源漂移拒绝通过 | `dist/cnweb-kernel-policy.log` |
| 每地址族防泄漏守卫 | 在独立 mount/network namespace 的真实内核中，分别移除 IPv6 规则与拒绝路由，均被健康检查拒绝；修复及释放通过 | `dist/cnweb-kernel-family-guards.log` |
| 真实历史版本升级 | 在 Debian 12 从实际 v0.4.0 标签源码构建隔离测试程序，运行旧服务后升级网站，再完整回滚；客户端文件逐字节、身份、证书、路由及续期入口保持；分享入口漂移在变更前被拒绝 | `dist/cnweb-historical040-web.log`；旧版使用 integration-test 以接受本地测试 CA，不冒充正式二进制公网升级 |
| 历史升级中断恢复 | 实际失败留下的外层材料/内层服务事务恢复通过，原网站及服务重新可用，日志清理完成 | `dist/cnweb-historical040-recovery.log` |
| 实际 socket 分流 | 固定 Xray 26.10.10，非 root + 仅 NET_RAW；CN 域名、CN IP、非 CN 域名/IP、双栈 TCP、UDP 多包往返、DNS 故障、停接口及删接口；tc 对原生口的 CN/DNS 匹配计数为零 | `dist/cnweb-packet-routing.log`；这是模拟 TUN 对端，不是 WARP 公网证据 |
| 官方原生后端 | LibreSpeed 真实上传/下载；OpenList 64 MiB 随机上传、完整下载摘要、Range、匿名拒绝、只读用户禁止写入、重启保留管理员和文件 | `dist/cnweb-native-services.log` |
| systemd 实际部署 | Nginx、Xray、LibreSpeed、OpenList、订阅服务实际启动，原有三协议握手通过；发现并修正首页 alias 与嵌套 proxy handler 问题 | `dist/cnweb-systemd-apply.log` |
| HTTPS 文件链路 | 实际受管理 Nginx → OpenList 的 64 MiB 上传/下载、摘要、Range、sandbox/attachment/nosniff、匿名拒绝、只读用户可读不可写通过 | `dist/cnweb-managed-web.log` |
| 服务启动失败恢复 | 用不可执行的测试后端触发自动重启状态；修复恢复逻辑后，旧 Nginx 原页面、服务状态及两级日志/维护标记完整恢复 | `dist/cnweb-systemd-recovery.log` |
| 浏览器真实测速 | 官方前端与 Go 后端完成下载、上传、延迟、抖动四项，OpenList 显示登录页；截图来自本地额外 HTTP 测试入口，数值不代表 VPS | 私有截图 `.local-deploy/cnweb-browser/` |
| 数据库维护与恢复 | 自动下载原生依赖、嵌套 API/下载/WebDAV 维护封锁、数据库复制前中断、备份后中断、应用失败自动恢复、持久提交后中断恢复均通过；恢复不重放旧数据库覆盖新写入 | `dist/cnweb-web-lifecycle.log` |
| 完成后漂移拒绝 | 实际数据库新增只读用户后，历史回滚拒绝覆盖，当前指针、服务、上传文件不变，无遗留恢复日志或维护标记 | 本地隔离环境 `nodectl-test/rollback-drift.log`，终端标记 `COMPLETED_ROLLBACK_DATABASE_DRIFT_REFUSED_WITHOUT_MUTATION_PASS` |
| 两内核三传输 | 固定 Xray 26.10.10、Mihomo 1.19.32；六组合 TCP HTTPS、UDP DNS、多包 UDP、强制 HTTP/3 QUIC、无直连泄漏及 24 个错误密钥/flow/缺失 Encryption/旧客户端负例通过 | `dist/cnweb-protocol-regression.log` |
| 上游配置校验 | 显式运行默认忽略的两个上游内核配置用例，固定 Xray/Mihomo 实际校验服务端与客户端配置通过 | `dist/cnweb-upstream-config-check.log`，2 个用例 |
| 正式发行与公开安装 | 两架构原生构建和发布成功；公开 raw 脚本与仓库逐字节一致，实际工具安装及最新版本解析为 0.5.0；清单 OIDC 身份验签、全部摘要、两个 ELF 架构、安装程序与归档一致，以及两架构标签来源证明均验证通过 | [v0.5.0 Release](https://github.com/i0721/dmit-node-config/releases/tag/v0.5.0)、[发行工作流](https://github.com/i0721/dmit-node-config/actions/runs/39141965544)；本地 `dist/release-v0.5.0/`、`dist/release-050-*-attestation.json`，不表示已在 ARM64 主机完成部署 |
| 真实双 WARP | 经单独授权，仅注册两个独立测试设备并复用凭据；两实例均验证 WARP trace。生产同源 Xray 分流模型完成 CN 域名 HTTPS、IPv4/IPv6 UDP DNS、非 CN 原生出口；停 CN 实例和删除接口后 CN 封闭，原实例及原生出口继续正常，原生口禁止目标计数为零、main 路由不变 | `dist/cnweb-real-warp.log`；`tests/integration/warp-real.sh`，仅本地一次性网络，不是生产验收 |
| 真实 CN systemd 生命周期 | 原生 Debian 12 优化构建，经完整 nodectl 事务管理两个已注册实例与网站：CN 启动后故障注入恢复、成功部署、双栈 UDP DNS 各四轮、CN HTTPS、非 CN 原生；停止 CN 后原 WARP 和原生出口可用，强杀自动重启并通过 doctor，卸载恢复原服务且保留数据库；默认路由与其他主表路由保持 | `dist/cnweb-real-cn-systemd.log`，`REAL_CN_SYSTEMD_LOCAL_FIXTURE_COMPLETED`；不把内核为新 TUN 地址自动生成的主机/链路路由误记为默认路由变更 |
| 公开 geodata 与私密材料隔离 | 实际部署中只读数据缓存仅两份文件、root 0644、单硬链接；两个 WARP 凭据仍为 0600，快照目录仍为 0700 | `dist/cnweb-cn-public-data-permissions.log` |
| 网站协议和 IP 证书 | 两个域名分别强制 H2、HTTP/3-only，严格核对返回协议；IPv4、IPv6 IP URL 均验证测试 CA 和证书后成功，不跳过证书验证 | `dist/cnweb-web-h2-h3-ip-tls.log` |
| 网站权限边界 | 私密配置 root 0600、网站及测速用户不可读；无 TTY 拒绝显示初始密码；两个后端仅 loopback、独立用户、空能力集、NoNewPrivileges、只读系统；敏感文件名请求只返回保留的旧站点 fallback，不返回配置 | `dist/cnweb-web-security.log` |
| 旧手动部署与事务 | Debian 13 隔离 systemd 环境：手动 render/check/takeover/apply、三握手、订阅权限、11 个应用故障点、恢复失败再恢复、回滚、卸载中断与漂移拒绝、域名证书/首页材料恢复通过；新功能缺省关闭 | `dist/cnweb-manual-lifecycle.log` |
| 向导与证书准备 | 当前问答脚本的取消不变更、一次确认部署和双客户端导出通过；显式运行隔离 ACME 用例 1 个通过，验证私钥复用及准备阶段不重载；ACME 使用本地替身和测试 CA，不是公网重新签发 | `dist/cnweb-manual-lifecycle.log`，`WIZARD_CANCEL_AND_SINGLE_SESSION_DEPLOY_EXPORT_PASS`、`ACME_STAGED_INSTALL_NO_RELOAD_AND_KEY_REUSE_PASS` |

## 本轮发现并处理的问题

- 单凭 route-get 成功或失败不能证明绑定接口 socket 的行为；实际流量证明 Linux on-link 构造可以绕过普通拒绝查表。增加 mark 终止规则，并在停止时显式停用专用接口，保持原生出口不动。
- 路由时解析域名不意味着 Freedom 后续不会调用系统 DNS；两个出站明确使用 ForceIP，并通过同一带标签 DNS 模型解析。
- Nginx 斜杠结尾 URI 的文件 alias 与嵌套 location 的 proxy/rewrite 继承不同。补实际浏览器、HTTPS 上传和维护子路由测试，不能以 nginx -t 代替。
- 服务自动重启时不能仅依靠稳定状态查询来决定是否停止；恢复先停止已加载服务，再读取稳定状态。
- 手动回归的镜像内旧问答脚本不能用于新增选项；改用当前脚本。单独重跑尾部检查时还需保留本地 DNS 与 HTTPS 探针，缺失时已恢复事务后补齐重跑。最终向导及显式 ACME 用例均实际通过，不以空过滤结果作为成功。
- 历史升级的外层材料恢复会先恢复订阅单元入口。仅在受校验的恢复日志与实际入口指纹一致时允许内层恢复继续，普通接管与摘要验证保持严格。
- CN 单元的严格只读挂载使命令输出捕获的默认临时目录不可写；临时文件改用该单元专用的 `/run/nodectl-cn-policy`，保持原有能力和挂载保护。
- 实测 systemd 无法通过 credentials 加载大型 GeoIP 数据。只将两份锁定摘要的公开 geodata 发布到独立只读缓存；私密快照仍为原权限，不公开配置、密钥或客户端材料。
- 加入大型上游程序和数据后，调试构建完整快照校验实测约 5.5 秒，超过订阅服务原有 5 秒就绪期限。保留该失败现场，改用原生 Debian 12 优化构建复验；优化程序实际恢复了该失败事务和原网站，没有放宽摘要或就绪检查。

## 仍待完成

1. 真实隧道已经通过上述有限流量检查；本次没有做 WARP 大流量测速，没有验证生产网络中的 Cloudflare 可达性，也没有验证新增 CN systemd 单元的完整生产部署。外层 MASQUE 使用 IPv4，隧道内 IPv4/IPv6 UDP 已实测，不将其表述为外层 IPv6 验收。
2. 生产续期仍待验证；新增 CN 单元的本地真实 systemd 全生命周期、历史版本接管、手动/向导及材料恢复已有上表证据。尚未重跑四个 amd64 系统与 ARM64 系统，不以下载了 arm64 文件冒充运行验收。此前追加的本地 CN 启动命令被自动审批拒绝；用户另行授权全新隔离 systemd 计划后才执行。前两轮启动失败与第三轮过严的主表比较记录均保留，最终优化构建完整流程通过，不以失败轮次冒充通过。
3. 生产切换预览、执行授权与上线验收仍待完成。公开材料已检查实际地址、域名及私钥标记，私密原始材料由忽略规则排除。本轮只读复核：生产运行 0.4.0，Debian 13，原四项服务活跃，新增 CN/网站后端未安装；原始记录私密保存。没有执行生产变更。

原始日志、凭据与隔离环境保留在受保护的本地目录；公开材料不包含实际 VPS 地址、域名、Token、私钥或客户端身份。外部设备注册、生产服务及资源变更均另行按具体预览授权。

## 真实 WARP 测试边界

两个一次性测试配置分别保存，模式为 0600；没有使用生产 WARP 或 DNS 凭据。测试先准备专用策略，再启动两个前台 usque，以固定 Xray 和 geodata 生成的生产同源分流模型发起请求。脚本不包含注册操作，只接受已授权的隔离 fixture，并监测原生接口禁止目标的 tc 计数；不能直接对生产网络运行。

本轮真实测试进程已停止，一次性网络已停用，私密注册材料和原始日志保留用于复现。注册预览原件与摘要保持不变，实际授权和执行结果另行记录；不能将预览原件中的“等待授权”视作最终执行状态。
