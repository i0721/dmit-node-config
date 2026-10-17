# Rust 原生化与 DMIT 自动部署实施计划

日期：2026-10-17。范围仅限 Python 移除与部署/升级自动化。

## 现状审计

生产程序为单体 Rust nodectl。Config/Secrets 为唯一配置模型，Schema 拒绝未知字段；render 使用既有模板，check 绑定二进制摘要、证书与参考客户端，apply/takeover/recover/uninstall 已实现受保护的事务日志和恢复。setup 只创建新配置，拒绝已有目录；upgrade 只升级签名 nodectl。本轮不得另建配置系统或绕过这些门禁。

主动 Python 依赖清单：scripts/check-docs.py（链接、锚点、部署标识泄漏）；check-workflows.py（Actions SHA）；download-xray.py（锁定下载与校验）；release-licenses.py（非开发依赖许可证图）；release-smoke.py（旧配置兼容）；tests/integration/probe.py（TLS 探针）。间接调用位于 check-all、release-package、test-installer、CI/release workflows、集成 Dockerfile/install-runtime/prepare/runtime。除此还需在结束时全文扫描确认无残留。

自动化缺口：系统依赖安装、保留配置的交互引导、Cloudflare API、双证书签发续期、可选 usque 注册、旧部署识别到应用的编排。生产已有服务的格式与版本尚待只读检查，不能假定兼容或静默生成新密钥。

## 设计与实施顺序

1. 将仓库工具迁入 Rust 辅助二进制，迁移所有调用点后删除 Python；保持摘要、许可、泄漏、兼容性门禁。TLS 测试服务由 Rust 提供，地址提取由 Rust 解析 ip JSON。辅助工具不能进入生产配置模型。
2. 在既有 CLI 中补齐部署编排；以有效 node.toml/secrets.json 为基础，仅显式输入覆盖；新配置使用 DMIT 模板并调用 config::init。预览与执行共用计划；DNS/ACME/软件准备与现有 render/check/takeover/apply 连接。所有远端安装固定来源或校验摘要。失败不报告上线，事务内失败使用原 recover；外部资源恢复单独记录。
3. 两项实现完成后统一验收：格式、单元/CLI、clippy、Schema、文档、工作流、发行兼容和许可证；Linux 隔离集成覆盖三协议、H2/H3、接管、故障注入与恢复、卸载和手动流程。
4. 生产仅先做只读盘点与生成预览，变更必须另获执行授权。真实 Cloudflare 截图只从真实 UI 获取并脱敏；不把示意图或模拟测试当真实验证。

## 不可降低的 DMIT 基线

TLS 1.3、H2/H3、443 XHTTP/WS、8443 XHTTP REALITY、本地 Nginx target、两个 UDS 及权限、80/15/5 客户端权重、5/2/3/1 指纹权重、域名 P-384 与 IP SAN 双证书、固定官方 Xray/usque/acme 来源、systemd 沙箱与 LoadCredential、独立 WARP 路由和默认直出。端口可显式调整，所有派生值仍由原模型渲染。

## 验收标准与证据

每项记录命令、环境、结果、限制。Python 文件与主动调用为零；等价安全门禁不得删除。默认配置与手动 render 产物一致。已有配置/密钥/证书不无故轮换，未知旧配置拒绝破坏性接管。DNS 仅指定名称、灰云、正确 A/AAAA；证书 SAN/密钥/有效期均通过原校验；WARP 前后默认路由不变；成功必须通过原 check/apply 的实际协议握手。备份及失败恢复可重试，敏感信息不进入日志、argv 或仓库。

未执行的生产变更、外部 CA/DNS/WARP 验证和缺失测试环境必须标记待验收，不能声明完成。
