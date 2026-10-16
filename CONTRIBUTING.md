# 贡献指南

使用 rust-toolchain.toml 的 Rust 1.99.1、Linux/WSL 与 Cargo.lock。先阅读 [架构](docs/architecture.md) 和 [约束](schemas/constraints.md)。通用能力优先复用 GitHub 开源库，不自行实现加密、协议或签名。

提交前运行 cargo fmt --check、cargo test --locked、cargo clippy --locked --all-targets -- -D warnings、Schema 对比、文档链接和安装器测试。涉及服务事务时必须运行隔离集成与失败恢复，不能在生产 VPS 注入故障。新增配置必须更新 Schema、示例、关联约束、迁移说明和测试。

Bug 提供版本、系统/架构、最小脱敏复现及预期/实际结果。不得提交真实 node.toml、secrets.json、客户端导出、DNS Token、WARP 或证书私钥。PR 说明问题、行为变化、测试证据、兼容性和恢复路径；保持单一目的。代码变更通过检查后由维护者审阅，提交说明使用明确动词。漏洞按 [安全政策](SECURITY.md) 私下报告。
