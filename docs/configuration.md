# 配置字段与迁移

唯一普通输入是 `/etc/dmit-node/node.toml`，凭据为同目录 secrets.json；目录 0700，文件 0600，归操作用户所有。setup 创建新输入，init 幂等补齐字段。编辑后 validate，不手动维护生成文件。

字段、默认值与规则见 [完整示例](../config/node.example.toml)、[JSON Schema](../schemas/node.schema.json)、[关联约束](../schemas/constraints.md)。node/ports 控制地址和三个独立端口，transport 控制随机路径与 UDS，tls/website/runtime/clients/warp 控制证书、站点、运行身份、客户端比例与独立 TUN。

公开示例仅可离线生成，上线拒绝文档域名/IP。IPv6 为空表示关闭，DNS/证书/监听同时省略。关闭 H3 时 alt_svc_max_age 必须为 0。80/15/5 为客户端分配权重，服务端不会随机分流。

## 兼容与迁移

schema_version 当前仅支持 1。`nodectl migrate` 校验 v1 后保持文件字节不变；未知版本拒绝，不能猜测结构或用默认值覆盖。以后新增迁移必须先备份，在副本变换与校验，再原子提交；失败保留原输入。

0.2 支持通过完整重生成校验的 0.1 模板 v1 快照；未知模板、生成器或内容拒绝。升级不轮换凭据或路径，轮换须显式 init --rotate。保留原二进制、输入与状态用于回退。
