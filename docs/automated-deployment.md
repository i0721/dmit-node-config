# DMIT 交互部署与升级

`nodectl deploy` 把现有依赖准备、配置模型、DNS、双证书、可选 WARP 与配置事务串联。`setup/init/render/check/apply` 的手动路径继续可用，模板和 Schema 不变。默认只预览；预览不创建配置、不申请证书、不改 DNS、不接管服务。

## 新主机

在 Debian/Ubuntu + systemd 的 DMIT 上以 root 运行。保留供应商控制台和第二个 SSH 会话。防火墙保留现有规则，操作者核实 HTTP TCP 80、HTTPS TCP/UDP 443、REALITY TCP 8443 及自定义 SSH 端口的公网可达性。程序验证本机地址和最终握手，不擅自重置防火墙。

```sh
nodectl deploy
# 或可重现的非交互预览（替换文档域名与 IP）
nodectl deploy --non-interactive --domain-suffix example.com \
  --ipv4 203.0.113.10 --ipv6 '' --warp false
```

新配置继承 `config/node.example.toml` 的全部 DMIT 基线。默认名称为 `dmit-qwq1`、`dmit-www`、`dmit-reality`；`--subdomain-prefix` 可调整前缀，完整名称可用 `--domain/--aliases/--reality-domain` 覆盖。`--http-port/--https-port/--reality-port` 修改同一 Ports 模型；REALITY target、监听与客户端地址自动派生。无 IPv6 时显式 `--ipv6 ''`；自动检测排除 tun/warp/docker/bridge 接口，多地址要求显式选择。

已有有效配置优先保留，包括 UUID、REALITY 密钥、short ID、传输路径、权重、证书路径和 WARP 凭据。只有显式参数或交互输入覆盖所选字段。可用 owner-only 的完整 `--input /root/candidate.toml` 提交手工配置，仍走相同校验与执行逻辑。

## Cloudflare 与证书

省略 `--dns-zone` 为手动 DNS，程序仍查询公开解析验证完整 A/AAAA。自动模式使用仅指定 zone 的 DNS Edit + Zone Read Token：

```sh
nodectl deploy --dns-zone ZONE_ID --acme-email operator@example.com \
  --accept-ca-terms
```

Token 从终端无回显读取；非交互环境用 `--token-file /root/cf-token`（root 所有、0600）。Token 不进入 argv 或项目配置；acme.sh 会按上游机制在私密账户目录保存续期凭据。浏览器登录状态不能替代 DNS API Token。Zone ID 必须为真实 32 位十六进制。

API 预览列出每个名称的 DNS 差异；仅操作该配置的 A/AAAA，强制灰云。相同记录不更新；更新用 PATCH 保留注释/标签/设置。遇到 CNAME、重复记录、禁用 IPv6 后仍有 AAAA 时拒绝猜测删除，先手动解决再预览。公开 DNS 尚未传播会失败并恢复已记录变更，不能把 API 写入成功当成解析验证成功。

更换主域名时，原主域名与 REALITY 域名自动保留为兼容别名。跨 zone 保留的旧域名不由新 zone 的 DNS API 修改，但仍校验公开解析。需要签发跨 zone SAN 时加 `--acme-discover-zones`，Token 必须只授权实际涉及的 zones 的 DNS Edit + Zone Read，让 dns_cf 为各 SAN 查找所属 zone；默认固定一个 Zone ID。预览会显示此选择，不能用仅新 zone 的 Token 冒充跨域验证。

保留已通过原有 SAN/受信 CA/密钥匹配/有效期校验的证书；仅对不合格的证书组签发。域名使用 DNS-01、P-384；IP 使用 HTTP-01、shortlived、EC-256、三天续期阈值。自动首次签发使用 Rust 临时 HTTP 挑战服务；若 80 已被占用，要求现有服务已正确提供 `/var/www/acme`，不停止其他网站。所有启用 IP 都要通过入口自检。IP 自动签发/续期要求 HTTP 80；自定义 HTTP 端口用 `--manual-certificates` 并自行管理公网挑战映射。

更换 SAN 需要重签时，兼容曲线的现有 TLS 私钥导入受保护的 ACME 缓存后复用；缓存密钥冲突时拒绝静默覆盖。DNS 传播默认最多等待 300 秒，可用 `--dns-wait-seconds` 在 1–900 秒内调整。

更换证书主名称而保留安装目录时，停用旧主名称的 ACME 续期任务，保留旧证书、密钥和缓存，防止旧任务覆盖新的 SAN 证书。跨 zone 自动发现会清除所选缓存中的旧 zone/account 提示；凭据仍受文件权限保护。自动模式先检查 ACME home 中是否存在其他项目证书，有则要求 `--manual-certificates`，避免改动共享账户自动化。

新账户的 `--accept-ca-terms` 表示操作者已审阅并接受 CA 条款；新 WARP 注册同理使用 `--accept-warp-terms`。已有 Let's Encrypt 账户复用原账户、联系方式与既有授权，不要求重新输入邮箱。程序不代替操作者接受新条款。`--manual-certificates` 保留人工签发与续期方式，最终仍执行原证书校验。

## 预览、授权与执行

预览包含配置、当前快照、服务状态、监听、文件摘要、默认路由、DNS 差异、执行阶段和恢复边界。审阅后将输出的完整 Plan SHA256 加到相同命令：

```sh
nodectl deploy --non-interactive --skip-dependencies --manual-certificates
# 审阅后：相同选项 + --execute --approve <完整 Plan SHA256>
```

计划摘要绑定主机和配置现状；发生漂移即拒绝执行，需重新预览。生产高影响操作必须由操作者授权具体计划。初次接管包自带或其他非托管入口时额外加 `--takeover`，调用原 takeover 保存入口备份后才 apply；默认不能覆盖未知入口。

系统准备仅按需安装缺少的软件，不执行整机升级。Nginx 不满足 H2/H3 时使用验证签名密钥的官方 apt 源；包维护脚本由临时 policy-rc.d 禁止自动启停服务。Xray 仅安装锁定版本并校验归档摘要；usque、acme.sh 从锁定提交构建/安装。缺少 usque 时按官方元数据校验并安装锁定 Go。`--skip-dependencies` 保留人工准备方式。WARP 使用独立 nativetun，不改默认出口；apply 与编排结束均验证 IPv4/IPv6 默认路由。

全新主机由本次安装产生的 Nginx 默认入口，只有通过 dpkg 配置摘要和再次校验后才自动接管；已有未知入口仍需 `--takeover`。

启用 WARP 时，执行阶段临时暂停现有独立 usque 服务，使用同一设备凭据启动仅绑定 `127.0.0.1` 的临时 SOCKS，通过受信 HTTPS trace 验证 WARP 出口，再结束探针并恢复原服务。避免同时用同一凭据运行两种模式；不通过给 TUN 添加默认路由来让测试通过。此短暂 WARP 维护在预览阶段中列出，DMIT 默认出口不变。

只有原有 check、apply、服务检查及三个真实协议握手全部成功才输出部署成功。ACME 自动模式在应用后配置现有 renew-hook 和 cron；不应将“尚未到期”当作一次真实续期验证。首次公网及续期周期验收仍按 [完整教程](deployment.md) 执行。

已知旧快照的端口、路径、域名和身份全部保留时，还必须用旧快照客户端完成三协议握手。用户显式修改这些兼容字段时，预览标为需要更新客户端，不能把此类变更宣称为完全兼容。配置续期钩子时避免子进程重复获取父部署锁：仅本次 install-cert 设置调用跳过子钩子，由持锁父进程直接执行原 renew 校验/reload；未来 cron 正常调用带锁 renew-hook。

## 已有服务升级与恢复

对配置有效的 0.1.0/0.2.0 部署直接预览 deploy，保留原始身份并重新生成候选。nodectl 自身的正式升级继续使用签名 `upgrade`，然后执行 deploy；两者没有替换关系。0.1 的快照仍由原完整重生成逻辑校验，缺少首次安装基线时仍拒绝猜测卸载。

没有原始配置、只有散落的 Nginx/Xray 服务时，先保留私密备份并把原 UUID、REALITY 密钥、路径、端口和证书导入有效 node.toml/secrets.json；不得用 init 重新生成身份来冒充兼容迁移。未知格式拒绝自动覆盖。已运行服务和客户端兼容性必须实际验证后再接管。

执行前在 state-dir/deployment-backups 下保护性备份配置、证书、WARP/ACME 材料、程序及修改的软件源；`deployment.json` 记录阶段及 DNS 请求意图/返回值。其他写命令在未恢复日志存在时被锁定。普通失败自动尝试恢复；崩溃或恢复失败时保留日志：

```sh
nodectl status --json
nodectl recover
```

recover 先核验备份并还原准备文件，再处理原服务事务、原快照及 DNS；需要恢复 DNS 时重新从终端读取 Token，不把 Token 持久化到日志。DNS 恢复检查并发漂移，重复恢复已还原的记录为幂等。未结束的准备子进程继承主机锁，恢复不能与其并发执行。不要删除事务日志或 backup 目录来绕过失败。新建用户/目录、apt 包数据库及 CA 已签发证书无法由配置快照撤销；保留备份和错误证据，需要时按包管理器/CA 流程处理。此边界在计划中明确展示。

恢复同时还原原服务的运行和开机启用状态。失败过程中产生的新证书、密钥及 WARP 材料先保存在该次备份的私密 `failed-material` 中，再恢复旧文件，便于人工核查已产生的外部资源。

## 上游依据

- [Cloudflare DNS 列表 API](https://developers.cloudflare.com/api/resources/dns/subresources/records/methods/list/)、[PATCH API](https://developers.cloudflare.com/api/resources/dns/subresources/records/methods/edit/)
- [Let's Encrypt shortlived profiles](https://letsencrypt.org/docs/profiles/)、[IP 证书可用性](https://letsencrypt.org/2026/01/15/6day-and-ip-general-availability)
- [acme.sh profile 选择](https://github.com/acmesh-official/acme.sh/wiki/Profile-selection)

本轮实施与实际验收记录在 [实施计划](defect/04-native-deployment-plan.md) 和 [验收报告](defect/06-native-deployment-acceptance.md) 中；文档能力描述不等于生产已执行。
