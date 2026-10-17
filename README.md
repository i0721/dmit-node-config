# dmit-node-config

Rust CLI `nodectl` 管理 Linux 裸机 Nginx、官方 Xray Core 与可选的独立 usque。单一配置源、确定性生成、脱敏差异、检查收据、真实握手、事务应用与恢复。用户客户端无需安装 WARP。

当前源码为 **0.3.1**；是否正式可用以 [Release](https://github.com/i0721/dmit-node-config/releases) 的签名资产为准。源码提交和本地测试不能替代正式资产及下载入口的实际验证。

## 适用范围与系统

面向能够管理 Linux、DNS 和证书的运维人员。x86_64 已验证 Debian 12/13、Ubuntu 24.04/26.04 的完整 systemd 生命周期。ARM64 的完整基线为 Debian 12；Ubuntu 24.04 也已通过，Debian 13 的运行日志通过但 CI 在清理阶段取消，Ubuntu 26.04 尚未完成，不能宣称全部 ARM64 系统兼容。离线配置生成不需 root；主机应用需 root。不使用 Docker 或面板承载节点服务。

nodectl 管理配置和服务事务，安装器管理 nodectl。[交互部署与升级](docs/automated-deployment.md) 的 deploy 预览并编排系统软件、DNS、双证书和 WARP 准备，再调用原配置事务；手动分层路径继续保留，render/apply 不暗中执行外部准备。WARP 不接管默认路由，Xray 默认 DMIT 直出。

## 快速开始

下载并执行根目录安装器，在同一次交互中完成安装、配置收集、变更确认、部署及客户端导出。目标主机无需安装 Rust；安装器自动准备并验证官方 cosign。正式 v0.3.1 资产不存在时会停止，不回退到候选文件。

```sh
curl -fL --proto '=https' --proto-redir '=https' \
  --connect-timeout 20 --max-time 1800 \
  -o dmitserverconfig-setup.sh \
  https://raw.githubusercontent.com/i0721/dmit-node-config/main/install.sh &&
bash dmitserverconfig-setup.sh
```

向导共用原有 Rust 配置和事务，保留手动 DNS、证书及高级分层流程。公网验收后再确认节点上线；旧手动参考见 [高级部署](docs/deployment.md)。文档将在正式资产和真实下载入口验证后补齐发行结果。

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

Schema v1；0.1/0.2 快照保留模板 1，0.3.0/0.3.1 使用模板 2，严格验证原始摘要。配置与凭据 owner-only，Xray 使用 systemd LoadCredential。首次 apply 保存原入口与服务状态；卸载还原这些事实，默认保留数据。0.1 安装缺完整基线时拒绝自动卸载，不能猜测原服务状态。

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

全量 GitHub 系统集成改为手动触发，避免重复本地已完成的测试。正式发行仅执行原生构建、归档检查、SBOM、签名及来源证明；被取消或未执行的矩阵不视为通过。
