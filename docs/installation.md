# 安装与交付

运行基线：x86_64 的 Debian 12/13、Ubuntu 24.04/26.04，ARM64 的 Debian 12；固定 Rust 1.99.1。其他 ARM64 系统按 [结果报告](defect/03-results.md) 的实际证据判断，Ubuntu 26.04 ARM64 尚未完成验收。两个架构均须原生构建及归档自检，不能用仅编译结果宣称全部系统兼容。

## 无 Rust 的安装

从 [Releases](https://github.com/i0721/dmit-node-config/releases) 获取版本对应的 install.sh，先审阅。准备 curl、tar、sha256sum 及 [官方 cosign](https://docs.sigstore.dev/cosign/system_config/installation/)。验证器本身必须来自可信供应链，不能下载未知同名程序。

```sh
sh install.sh --version v0.2.0 --dry-run
sudo sh install.sh --version v0.2.0 --execute
nodectl --help
nodectl doctor
```

版本必填，不隐式跟踪 latest。先验证 SHA256SUMS 的 Sigstore 签名，固定 GitHub OIDC issuer 与本仓库 release.yml 的标签身份，再校验归档，且只抽取 nodectl。新二进制版本、help、schema 和已有配置 validate 通过后才原子替换。旧程序保存在 `/usr/local/lib/nodectl/versions/<sha256>`。不提供跳过签名参数。

离线资产放在绝对目录，用 `--bundle-dir /absolute/directory`；仍验证签名和摘要。dry-run 不下载、不创建目录。执行安装需要对目标目录及其父路径的安全控制，默认通过 root 安装。

## 源码与运行依赖

```sh
cargo test --locked
cargo build --release --locked
sudo install -m 0755 target/release/nodectl /usr/local/bin/nodectl
```

源码信任来自已审阅提交和 Cargo.lock，不能冒充正式 OIDC 资产。系统组件安装见 [部署参考第 4 节](deployment.md#4-服务器软件与防火墙)。Nginx >=1.25.1，SSL/H2/H3；其他版本见 versions.lock.toml。Xray v26.10.10 为预发布版本，固定摘要不是稳定性保证。运维人员管理依赖升级，升级后重做 check 与完整握手，失败恢复原程序。

## 发布资产

两个架构分别提供 tar.gz，包含 nodectl、LICENSE、THIRD_PARTY.md、依赖许可证、CycloneDX SBOM 和构建信息。SHA256SUMS 覆盖下载资产，Sigstore 签名绑定精确版本工作流；GitHub 构建证明关联源码提交和工作流。另可执行 `gh attestation verify <archive> --repo i0721/dmit-node-config`。工作流文件存在不等于 Release 已发布；发布后必须核验真实资产及证明。
