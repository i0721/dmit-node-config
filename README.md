# dmit-node-config

按 [plan.md](plan.md) 实现的 Rust 管理工具 `nodectl`。Linux 裸机运行 Nginx、原始 Xray Core 和独立 usque；不使用 Docker 或管理面板。

普通参数源为 `node.toml`，凭据放在 `secrets.json`。三类入口、两类 Nginx 虚拟主机、systemd 单元和客户端分配清单均由同一输入生成。WARP 只启动 `nativetun`，不添加路由、不改变 Xray Freedom 出站。

首次部署请按 [完整部署教程：Cloudflare DNS、证书、服务器与 WARP](docs/deployment.md) 顺序操作。教程使用 `dmit-qwq1.yushulinfeng.com`、`dmit-www.yushulinfeng.com`、`dmit-reality.yushulinfeng.com`，涵盖 DNS/API Token、两套证书首次申请及自动续期、软件安装、WARP TUN 调试、上线验收和故障恢复。本文保留项目命令的简要参考。

配套的 [截图与图解导航](docs/visual-guide.md) 包含 4 张真实 Cloudflare 表单/模板截图、2 张工程流程图和 [部署记录模板](docs/resources/deployment-record.template.md)。草稿截图均明确标注未提交。

## 构建与验证

```sh
cargo build --release --locked
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

安装 `target/release/nodectl` 到 `/usr/local/bin/nodectl`。实际主机操作只支持 Linux + systemd，要求 root；离线配置生成和单元测试不要求 root。Windows 上建议通过 WSL 的 Linux 文件系统工作，保护目录必须能真实保留 Unix 权限。

通用能力复用开源库，依赖版本和摘要锁定在 `Cargo.lock`。GitHub 地址和许可证见 [THIRD_PARTY.md](THIRD_PARTY.md)；CI 使用 [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) 检查许可证及安全公告。未重新实现协议栈、密码算法、配置解析、模板引擎或差异算法。

## 首次准备

按方案准备受信任的 Xray、支持 H2/H3 的 Nginx、usque、OpenSSL、curl、iproute2、util-linux 和 systemd。`versions.lock.toml` 固定 Xray v26.10.10，包含已核对的 Linux amd64 官方归档摘要。工具不会下载或升级这些程序，也不会调用 DNS API、签发证书或注册 WARP。

默认路径的准备示例：

```sh
useradd --system --no-create-home --shell /usr/sbin/nologin xray
install -d -m 0750 -o xray -g www-data /var/log/xray
install -d -m 0755 /var/log/nginx /var/www/html /var/www/acme
install -d -m 0700 /etc/usque /etc/ssl/dmit/domain /etc/ssl/dmit/ip
nodectl init
```

`www-data` 用户和同名组须已存在。Nginx master 以 root 读取证书，worker 以配置用户运行。Xray 主组使用 Nginx 组，保证创建 UDS 为 `xray:www-data 0660`；仅添加 supplementary group 不能保证新 socket 的组所有权。Xray 通过 systemd `LoadCredential` 读取配置，快照目录仍保持 root 专用。

`init` 调用当前安装 Xray 的 `x25519` 生成私钥，使用 x25519-dalek 派生公钥；生成三个不同 UUID、Short ID 和三个随机路径。重复执行只补缺失值，轮换需显式指定；已有占位符或非法值会报错。

编辑 `/etc/dmit-node/node.toml` 的真实域名、IP、端口和路径。配置文件为 root 所有、`0600`，目录为 `0700`。文档地址可以离线渲染，上线检查会拒绝。无 IPv6 时设 `ipv6 = ""`；关闭 H3 时同时设置 `http3 = false`、`alt_svc_max_age = 0`。

准备两套有效证书，域名证书要求 ECDSA P-384。私钥须为 root 所有、`0600`，SAN 覆盖生成清单，通过系统 CA 验证，剩余有效期至少一天。`render` 的 `operations.json` 含 DNS 记录、证书申请及安装参数；DNS_PROVIDER 是待选择的 acme.sh DNS 插件。首次申请 IP 证书时尚未运行 HTTP 服务，可用 acme.sh standalone；后续使用 `/var/www/acme` webroot。HTTP-01 始终要求外部 80 端口可达，即使修改了本机监听端口。

启用 WARP 时，先单独执行 `/usr/local/bin/usque -c /etc/usque/config.json register`，结果为 root 所有、`0600`。普通 render/apply 不重新注册。

## 日常流程

```sh
nodectl validate
release_id=$(nodectl render)
nodectl diff "$release_id"
nodectl check "$release_id" \
  --client /usr/local/bin/xray \
  --client-version 26.10.10 \
  --client-sha256 "$(sha256sum /usr/local/bin/xray | cut -d ' ' -f 1)" \
  --probe-url https://www.cloudflare.com/cdn-cgi/trace
nodectl apply "$release_id"
nodectl export-clients --output /root/exports
```

参考客户端要求明确版本和 SHA256，先从受信任构建或官方摘要确认其来源。probe URL 可替换为自有的稳定 HTTPS 地址。导出为 Xray 原生 outbound 和分配清单，可并入客户端 `outbounds`。XHTTP 默认有 11 个指纹分配位置，比例 5/2/3/1；传输总权重 80/15/5。WS 使用 HTTP/1.1 ALPN，stream-one TCP 客户端使用 H2；Nginx 独立提供 H3 并发送 Alt-Svc。

`render` 原子生成 `/var/lib/dmit-node/releases/<摘要>/`，目录 `0700`、文件 `0600`。相同输入、模板及工具版本产生相同 ID。manifest 记录源摘要、模板/工具版本和每个文件的 SHA256；校验还会重新生成配置比对，拒绝额外文件、符号链接及篡改。旧快照要求原工具版本，升级前应保留旧二进制。

`diff` 隐藏新旧 UUID、私钥及 Short ID；工具报错不直接显示第三方程序的原始输出，以防凭据进入日志。

`check` 不切换配置、不重启服务。它核实证书期限、SAN、信任链和私钥匹配、服务用户访问及网站路径冲突，并运行实际 Xray、Nginx 和 systemd 检查。候选没有读取线上配置的 include。检查记录二进制、证书和 WARP 文件摘要；它们改变或重新检查失败后，候选无法应用。

`apply` 锁定进程、重做预检查、保存入口与服务状态，然后切换受管理入口。如果首次发现已有非本项目配置，需显式接管：

```sh
nodectl takeover "$release_id"
# 在 takeover/<release-id>/ 查看 diff.txt 和备份。
nodectl apply "$release_id"
```

`takeover` 保存原内容和摘要差异，本身不修改入口；原内容再次变化会使授权失效。只有单元变化才 daemon-reload；按受影响组件重启 Xray、reload Nginx，监听或单元变化则重启 Nginx。usque 仅在配置、凭据、启用状态变化或未运行时处理。

应用后检查服务、IPv4/IPv6 TCP、UDP、UDS 权限和三类真实握手。固定的参考客户端监听临时本地 SOCKS 端口，经节点访问检查 URL；默认路由变化也判失败。全部通过才更新 current/previous。导出始终来自成功版本。

## 回滚与恢复

```sh
nodectl rollback
nodectl recover
nodectl renew-hook
```

回滚仍须通过当前证书、程序及握手检查。应用失败会恢复入口、单元、原服务启用/运行状态，再检查上一版本；首次部署恢复原入口和服务状态。多服务切换可能短暂断连。

崩溃或恢复失败时保留 `transaction.json`、`backups/`，阻止后续写入。`recover` 在同一把锁下重试恢复。恢复失败明确报错并写入 `recovery-required.txt`，不宣布成功。外部更新的证书、WARP 凭据和网络不会被覆盖；它们导致旧版检查失败时，应先恢复这些外部条件，再 recover。不要直接删除未恢复的事务日志。

自动保留最近 5 个成功版本，始终保护 current/previous。候选、接管和故障备份保留用于诊断，按主机保密策略另行清理。

acme.sh 续期成功后调用 `/usr/local/bin/nodectl renew-hook`，检查证书并仅 reload Nginx。首次尚无 current 时 hook 会报错，应先安装证书并完成首次 apply；新证书无法覆盖旧版本 SAN 时，旧版本回滚检查会失败。

## 开发与验收

`nodectl schema` 输出 Rust 模型生成的 JSON Schema，提交版本见 [schemas/node.schema.json](schemas/node.schema.json)；关联约束见 [schemas/constraints.md](schemas/constraints.md)。

可通过 `--config-dir`、`--state-dir` 指定绝对路径离线使用；`init --example` 指定 TOML 模板，Xray 必须已安装。显式轮换例子：`nodectl init --rotate ws_uuid,transport.ws.path`。允许五个 secrets 字段和三种传输的 path 字段。

测试覆盖参数边界、联动、幂等、脱敏、快照篡改、锁及恢复分支。额外的官方 Xray 兼容测试：

```sh
XRAY_TEST_BINARY=/path/to/verified/xray \
  cargo test --locked upstream_xray_accepts -- --ignored
```

该测试使用真实 Xray 生成密钥、核对公钥并验证服务端及全部客户端配置。它不代替真实节点上的 Nginx、systemd、证书、QUIC 和三类联网握手验收；这些由 check/apply 强制执行。REALITY 到本地 Nginx target 保留方案中的实验性质，握手失败不标记成功。
