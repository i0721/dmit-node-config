# 项目设计入口

正式设计见 [架构](docs/architecture.md) 与 [决策记录](docs/decisions/0001-lifecycle-boundaries.md)。产品化依据 [缺口审计](docs/defect/01-plan.md) 和 [实施方案、技术设计与任务计划](docs/defect/02-implementation-plan.md)。本文件不再保存对话或重复的手写运行配置。

设计约束：Rust CLI 管理单一配置源；Linux + systemd 裸机运行 Nginx、官方 Xray Core 和独立 usque。443 提供 VLESS XHTTP/WS，8443 提供 XHTTP REALITY，本地 target 为 Nginx。TLS 1.3、H2/H3、两个 UDS；客户端权重 80/15/5，XHTTP 指纹权重 5/2/3/1。usque 不接管默认路由，Xray 默认直出。配置生成确定，凭据私密，应用可验证与恢复。

安装、发布、升级、卸载与运维以 [README](README.md) 和分层手册为准。公开示例使用文档占位符，真实部署记录不得提交仓库。