# CN 独立出口与测速、文件网站

本页描述 **0.5.0 开发中的可选配置**，不是已发布版本或生产验收声明。正式安装入口继续以 README 标注的 Release 为准。实施和验收状态见 [实施计划](defect/11-cn-warp-web-services-plan.md) 与 [验收记录](defect/12-cn-warp-web-services-acceptance.md)。

## 启用和保留配置

安装器仍只调用 Rust 部署向导。向导增加 CN 独立 WARP、测速及文件网站两个选择，已有有效值优先；旧配置缺少新节时保持原行为，不静默启用。高级非交互预览可使用 `nodectl deploy --non-interactive --cn-warp true --web-services true`，并继续提供适合该主机的现有证书、DNS、审批参数。预览不会注册设备或修改服务。

手动与自动部署使用同一份 `node.toml`，可增加以下节；完整旧平台配置继续保留，不能用这两节替换原文件：

```toml
[cn_warp]
enabled = true
interface = "warp-cn"
config_file = "/etc/usque-cn/config.json"
routing_table = 5271
rule_priority = 15271
socket_mark = 5271
geodata_dir = "/var/lib/nodectl-geodata/202610172205"

[web_services]
enabled = true
librespeed_port = 8989
openlist_port = 5244
openlist_data_dir = "/var/lib/nodectl-openlist"
openlist_files_dir = "/var/lib/nodectl-openlist-files"
download_size_mib = 256
```

`rule_priority` 同时保留紧邻的下一优先级，本例为 15272。冲突时停止，不清空他人的策略。新 CN 实例必须使用单独注册的设备、访问凭据及私钥，不能复制原实例的配置；两账户分配到相同的私有隧道地址并不代表身份相同。

## CN 与 DNS

`geosite:cn` 或 `geoip:cn` 命中走绑定 `warp-cn` 的 Freedom 出站，其余走原生 direct。CN 出站带专用 mark；direct 只设置 DNS 解析策略，不带 mark 或接口绑定。两者通过 Xray 的同一套带标签 DNS 解析，避免出站再次调用系统解析器。CN DNS 使用 223.5.5.5，禁止回退；非 CN 使用 1.1.1.1。

GeoIP/GeoSite 的日期、来源与摘要固定在 [版本锁](../versions.lock.toml)，复制到不可变快照。旧快照验证使用快照自身的数据并核对锁定摘要及完整文件清单，不依赖缓存目录继续存在。

Xray 运行时读取 `/var/lib/nodectl-cn-assets/releases/` 下按摘要命名的公开数据副本，仅包含两份 geodata。发布和健康检查均核对摘要、文件清单、root 所有权及只读权限；私密快照权限不变。该目录不通过网站提供，卸载保留可重新生成的缓存。

仅专用表中写默认路由；main 默认路由保持原样。拒绝路由和终止 mark 规则共同保护策略，停止时还会停用专用接口，处理 Linux 绑定 oif 可以构造 on-link 路由的情况。隧道不存在时，Xray 的接口绑定必须失败，不重试原生出口。严格 RPF 会阻断这种非对称出站时，仅在新建专用接口设置 loose RPF；原配置已允许时不写 sysctl，不修改 native、all 或 default 的过滤设置。

最小能力设置要求 Linux 5.17 或更新版本，Xray 仅获得 `CAP_NET_RAW`；较旧内核停止并说明要求，不自动扩大能力。依据为 [Linux socket 文档](https://man7.org/linux/man-pages/man7/socket.7.html)。

## 网站、凭据与数据

TLS 主域名提供 Landing Page、`/librespeed/` 与 `/downloads/test.bin`；files/REALITY 名称提供 OpenList。两站保留原有传输路径、域名证书、H2/H3 和 REALITY 回落。原网站目录不删除，生成页面放在独立的公开静态白名单目录中。

LibreSpeed 1.1.6 与 OpenList 4.2.7 使用上游原生二进制，归档、架构与可执行文件均验摘要。服务以两个独立非登录系统用户运行，仅监听 loopback。测速使用官方 JavaScript/worker 与 Go 后端，关闭测速遥测和第三方 IP 查询。测速会消耗实际流量；默认下载文件为 256 MiB，创建前检查可用空间。

OpenList 使用本地 SQLite 数据库及独立文件目录。首次随机管理员密码通过私有终端显示，也可在 root 终端执行 `nodectl web reveal` 取回初始值；既有管理员密码不会重置，后来在 OpenList 改过的密码优先。`nodectl web status` 只显示站点及受保护的保存位置，不打印密码。`nodectl status` 和 `doctor` 纳入新增服务及维护状态。

匿名默认不能读写存储。上传文件下载强制 attachment、nosniff 和 sandbox 响应头；这会限制 HTML 等文件的站内预览执行。节点登录界面仅位于主域名 `/nodes/`，files 的该页面跳转到主域名，节点 API 不在文件站提供；两个受保护的订阅下载路径保留，凭据校验不改变。用户自定义 OpenList 存储和权限不因重复部署重新创建。

## 备份、维护和恢复

网站部署使用恢复格式 3：先持久写入维护标记及服务原状态，暂停 Nginx/OpenList/LibreSpeed，随后复制数据库和私有配置，避免在线复制 WAL 数据库。登录、下载、WebDAV 等嵌套路由同样受维护标记约束。中断后 `nodectl recover` 依日志继续恢复；数据库材料恢复前不能先重启旧服务。

备份涵盖程序、服务入口、私有配置、数据库、初始凭据和原存储根目录权限。用户上传文件不作为代码部署副本复制或删除；恢复时只还原存储根目录元数据。已有上传数据始终保留。已经恢复服务并开放写入后，恢复最终清理不会再次覆盖数据库。

完成后的历史回滚会核对材料漂移；用户已修改数据库时拒绝静默覆盖，须先作运维备份和明确的数据恢复决策。卸载恢复原服务入口，保留数据库、上传文件、凭据、配置、证书和审计材料。已安装的软件包及非登录系统账户也保留。

上游许可与源码：[LibreSpeed / LGPL-3.0](https://github.com/librespeed/speedtest-go/tree/v1.1.6)、[OpenList / AGPL-3.0](https://github.com/OpenListTeam/OpenList/tree/v4.2.7)、[usque v4.2.1](https://github.com/Diniboy1123/usque/tree/v4.2.1)。后端按锁定地址下载，未重新打包进 nodectl 发行归档。
