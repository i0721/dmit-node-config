# 架构与安全边界

nodectl 是 Rust 配置与变更管理器。输入是唯一的 node.toml 与独立 secrets.json；输出包括 Nginx、Xray、systemd、操作清单及客户端分配。安装器只处理 nodectl，普通 render/apply 不操作 DNS、CA 或 WARP 账户。

## 数据流

`输入 → validate → 确定性快照及 manifest → diff → check 收据 → apply → 服务和握手验收 → current/previous`。

443/TCP 的 Nginx 提供 TLS 1.3、H2，分别经两个 UDS 转发 XHTTP/WS；443/UDP 提供 H3。8443/TCP 为 XHTTP REALITY，本地 target 从 HTTPS 端口派生。usque 独立运行 nativetun，不接管默认路由或改变 Xray Freedom 出站。

![网络示意](assets/deployment/network.svg)

## 事务与恢复

check 校验证书、依赖摘要、真实配置与服务用户权限。apply 在独占锁下重查，备份入口和服务状态，写 transaction.json，切换入口，检验服务、TCP/UDP/UDS、三类握手和默认路由。成功才提交指针；失败恢复全部入口及原服务状态，恢复失败保留记录并禁止写入。

首次应用前保存 installation.json，后续应用不能覆盖。uninstall 验证原始备份与当前入口后，保存当前状态到 uninstall.json，再还原首次安装前事实；失败撤销卸载，中断由 recover 恢复安装。缺失历史基线不能猜测。证书、WARP、输入、快照与程序保留。

## 版本和凭据

快照绑定 schema、模板、生成器和输入/输出摘要，并完整重新生成比对。0.2 兼容通过重生成验证的 0.1 模板 v1；未知版本拒绝。Xray 使用 systemd LoadCredential，快照/凭据 owner-only；Nginx root master 读取私钥。第三方原始输出不进入普通错误。

见 [安全](security.md)、[配置](configuration.md) 和 [设计决策](decisions/0001-lifecycle-boundaries.md)。
