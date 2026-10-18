# 独立 CN WARP 与测速、文件网站实施计划

日期：2026-10-18。状态：审计、设计与本地实施完成；实际验收、发行及生产边界见 [验收记录](12-cn-warp-web-services-acceptance.md)。范围仅为用户要求的两个任务及必要生命周期适配。

## 已审查的现状

- 当前正式源码为 0.4.0、模板 3。Xray 仅有 direct/block 出站，没有 geosite/geoip 及分流 DNS。原 WARP 运行 `usque-warp`，配置 `/etc/usque/config.json`，不接管默认出口。
- Nginx 将两个名称置于同一个域名 server，已有 XHTTP/WS、IP TLS、H2/H3、ACME 和 `/nodes/`。REALITY 将未认证握手转交本机 HTTPS 端口。新增 files 虚拟主机必须继续共享域名证书，正确响应 files SNI，并保留代理路径及受保护订阅路由的明确边界。
- host 的旧事务固定五个入口、三个服务，0.4 增加可选 sharing 成员。deploy 另有私密材料、DNS、证书、首页与服务日志；新单元和程序/数据备份必须纳入这些机制，不能绕开审批和恢复。
- 输入脚本为用户指定的本地 `tmp/usque_v0.2.0.sh.txt`，与先前读取的下载副本摘要相同。已审查其 register、run_daemon、add_routes、cleanup_routes：固定 tun0/table250，全局清理规则，缺少原生出口时写 main default，IPv6 replace 还增加 from-all 规则。只参考注册、独立配置及 exec 前台 nativetun，不执行脚本，也不复制这些全局路由操作。参考脚本不进入发行包。

## 目标设计

1. 新增可缺省的 CN 分流配置与网站服务配置；旧配置缺省保持原行为，向导显式询问。平台专用旧字段、身份、端口、路径、权重和双证书不变。
2. 新增独立 `usque-cn` 实例，独立 config、TUN、状态及 systemd 单元，复用已锁定的 usque 4.2.1。配置冲突、TUN/规则/策略表占用须在变更前拒绝，不清空他人资源。
3. Xray 的 CN freedom 出站同时绑定 TUN interface 与独立 fwmark。仅该 mark 查询专用表；表中保留 unreachable 默认兜底，隧道路由优先于拒绝路由；不得向 main 表写默认路由，不改变 SSH/原 WARP。切换和停止顺序必须保证 Xray 仍可能发送 CN 流量时拒绝规则存在。
4. `geosite:cn` 和 `geoip:cn` 走 CN WARP，其余 direct；分流 DNS 使用 Xray 内置 DNS，CN 解析请求走指定 WARP，禁用 CN 查询向其他解析器回退。锁定 GeoSite/GeoIP 来源、版本及 SHA256，纳入快照、校验、升级与恢复；缺文件或摘要不符不启动新配置。
5. Speedtest 域名提供中性现代 Landing Page、LibreSpeed 测速入口、大文件下载及 files 入口。复用官方 LibreSpeed 前端/Go 后端，不自行模拟测速结果；关闭遥测与外部 IP 查询，后端仅 loopback。下载提供真实可 Range 读取的本地测试文件，预览磁盘空间及流量影响。
6. files 域名单独反代 OpenList 原生程序，loopback 监听、专用非登录用户、独立持久化目录和本地存储。保留用户数据库、文件、配置和管理凭据；首次管理员密码只在私密机制取回。默认禁匿名写入，验证权限后开放相应用途，不用公开默认密码。
7. Nginx 大文件反代禁用请求/响应缓冲并使用合理超时，保留 TLS/H2/H3；files SNI 仍能作为 REALITY 目标。限制管理及测速端点滥用，不影响 `/nodes/` 的限流、禁缓存与日志保密。
8. 同一 Rust 配置、render/check/apply 与 deploy/recover/rollback/uninstall 执行链管理新增依赖、服务、配置和数据备份。旧模板 1/2/3 冻结；新模板 4 与开发版本 0.5.0 独立映射，未知版本和完整性错误继续拒绝。新增快照不能冒充 0.4 格式。

## 上游依据及版本准备

- [usque v4.2.1 源码](https://github.com/Diniboy1123/usque/tree/v4.2.1)：nativetun 支持独立 interface-name、重连及前台运行；不使用 insecure。
- [LibreSpeed Go](https://github.com/librespeed/speedtest-go)：原生 Go 后端与官方测速前端，准备锁定 v1.1.6 及资产摘要。
- [OpenList](https://github.com/OpenListTeam/OpenList)：准备锁定 v4.2.7 原生 Linux 两架构资产及摘要，审查配置和本地存储 API。
- [Loyalsoldier geodata](https://github.com/Loyalsoldier/v2ray-rules-dat)：准备锁定 202610172205 发布的数据与摘要，不使用可变 latest URL 作为运行时输入。
- Xray 的实际配置与 socket 行为以已锁定 26.10.10 源码和二进制验证为准，不以最新版文档代替。

## 实施顺序与验收

| 顺序 | 工作 | 必须取得的证据 |
| --- | --- | --- |
| 1 | 锁定依赖、模型、模板兼容 | 下载摘要/上游来源；旧快照完整校验；新字段校验及冲突拒绝 |
| 2 | CN 路由、DNS、独立实例 | 两实例同时运行；CN 域名/IP TCP/UDP 实际经 WARP；非 CN 原生；IPv4/IPv6 默认路由与 SSH 不变 |
| 3 | 失败封闭 | 断隧道、杀进程、接口删除、DNS 故障及重启顺序下抓包无 CN 直连；非 CN 仍可用 |
| 4 | 网站与文件服务 | 浏览器真实上传/下载、延迟/抖动测量；OpenList 登录、上传、下载、Range/大文件和匿名权限拒绝 |
| 5 | 生命周期 | 向导一次配置、变更预览、现有自定义配置保留、故障注入、完整回滚和卸载恢复；数据库备份一致性与磁盘空间门槛 |
| 6 | 全量回归与报告 | Python 零依赖、锁定内核配置/协议、订阅访问、REALITY/files 回落、H2/H3/IP TLS、旧 WARP；实际限制明确记录 |

不在 VPS 引入 Docker、Python、面板或新的公开后端端口。普通本地实施无需确认。真实 WARP 账户注册、生产 DNS/证书、路由、服务及网站切换均须先给具体预览并取得授权；没有外部实际证据的项目保持未验收，不将本地模拟流量标成真实 WARP。
