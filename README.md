# dmit-node-config

Rust CLI `nodectl` 管理 Linux 裸机 Nginx、官方 Xray Core 与可选的独立 usque。单一配置源、确定性生成、脱敏差异、检查收据、真实握手、事务应用与恢复。用户客户端无需安装 WARP。

当前工作树为 **0.2.0 候选版**；正式发布和系统矩阵以 [Release](https://github.com/i0721/dmit-node-config/releases) 及 [缺口验收记录](docs/defect/03-results.md) 为准。当前源码不能替代正式资产和签名的实际验证。

## 适用范围与系统

面向能够管理 Linux、DNS 和证书的运维人员。x86_64 已验证 Debian 12/13、Ubuntu 24.04/26.04 的完整 systemd 生命周期。ARM64 的完整基线为 Debian 12；Ubuntu 24.04 也已通过，Debian 13 的运行日志通过但 CI 在清理阶段取消，Ubuntu 26.04 尚未完成，不能宣称全部 ARM64 系统兼容。离线配置生成不需 root；主机应用需 root。不使用 Docker 或面板承载节点服务。

nodectl 管理配置和服务事务，安装器管理 nodectl。[交互部署与升级](docs/automated-deployment.md) 的 deploy 预览并编排系统软件、DNS、双证书和 WARP 准备，再调用原配置事务；手动分层路径继续保留，render/apply 不暗中执行外部准备。WARP 不接管默认路由，Xray 默认 DMIT 直出。

## 快速开始

从正式 Release 获取并审阅 install.sh，准备官方 cosign、curl、tar 和 sha256sum。目标主机无需安装 Rust。

```sh
sh install.sh --version v0.2.0 --dry-run
sudo sh install.sh --version v0.2.0 --execute
nodectl setup --non-interactive --domain node.example.com \
  --reality-domain reality.example.com --ipv4 203.0.113.10
# 将占位符替换成真实输入，审阅后加 --execute 创建配置
nodectl doctor
nodectl status
```

按 [首次部署](docs/getting-started.md) 准备依赖、DNS 和双证书，再 validate → render → diff → check → takeover（已有入口时）→ apply。公网验收后配置自动续期和监控。不能把 setup 成功当成节点已上线。

## 稳定命令与文档

| 任务 | 命令/手册 |
| --- | --- |
| 安装、签名验证、两个架构 | [installation](docs/installation.md)、install.sh |
| 首次成功路径 | [getting-started](docs/getting-started.md)、setup、doctor |
| 交互自动部署、保留配置升级 | [automated-deployment](docs/automated-deployment.md)、deploy |
| 字段与 schema 迁移 | [configuration](docs/configuration.md)、validate、migrate |
| 预览与事务 | render、diff、check、takeover、apply |
| 状态、升级与卸载 | status、upgrade、uninstall；默认预览 |
| 恢复、续期、监控、备份 | [operations](docs/operations.md)、rollback、recover、renew-hook |
| 按症状排错 | [troubleshooting](docs/troubleshooting.md) |
| 架构、威胁模型 | [architecture](docs/architecture.md)、[security](docs/security.md)、[ADR](docs/decisions/0001-lifecycle-boundaries.md) |
| 完整参考与通用图解 | [deployment](docs/deployment.md)、[visual-guide](docs/visual-guide.md) |
| 贡献与漏洞 | [CONTRIBUTING](CONTRIBUTING.md)、[SECURITY](SECURITY.md)、[行为准则](CODE_OF_CONDUCT.md) |

## 兼容与安全

schema/template v1；0.2 重生成验证兼容的 0.1 快照。配置与凭据 owner-only，Xray 使用 systemd LoadCredential。首次 apply 保存原入口与服务状态；卸载还原这些事实，默认保留数据。0.1 安装缺完整基线时拒绝自动卸载，不能猜测原服务状态。

工具升级不升级代理依赖。Xray 锁定的 v26.10.10 为预发布，必须实机验收，不把锁定摘要当成稳定性承诺。失败时保留日志与备份，先 recover；不要删除未恢复事务。

## 开发

```sh
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
```

固定 Rust 1.99.1 和 Cargo.lock。复用 GitHub 开源库，依赖及许可证见 [THIRD_PARTY](THIRD_PARTY.md)。历史设计入口 [plan.md](plan.md) 已指向正式架构；缺口任务见 [实施方案](docs/defect/02-implementation-plan.md)。

项目主动开发/构建/测试/发布流程不再需要 Python。仓库检查、锁定下载、许可证收集、发行兼容测试及 TLS 测试探针由 Rust `repo-tools` 提供；入口为 `bash scripts/check-all.sh`。
