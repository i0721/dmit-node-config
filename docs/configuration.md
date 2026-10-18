# 配置字段与迁移

唯一普通输入是 `/etc/dmit-node/node.toml`，凭据为同目录 secrets.json；目录 0700，文件 0600，归操作用户所有。setup 创建新输入，init 幂等补齐字段。编辑后 validate，不手动维护生成文件。

字段、默认值与规则见 [完整示例](../config/node.example.toml)、[JSON Schema](../schemas/node.schema.json)、[关联约束](../schemas/constraints.md)。node/ports 控制地址和三个独立端口，transport 控制随机路径与 UDS，vless/tls/website/runtime/clients/warp 控制证书、站点、运行身份、客户端比例与独立 TUN。

公开示例仅可离线生成，上线拒绝文档域名/IP。IPv6 为空表示关闭，DNS/证书/监听同时省略。关闭 H3 时 alt_svc_max_age 必须为 0。80/15/5 为客户端分配权重，服务端不会随机分流。

## 兼容与迁移

schema_version 当前仅支持 1。`nodectl migrate` 校验 v1 后保持文件字节不变；未知版本拒绝，不能猜测结构或用默认值覆盖。以后新增迁移必须先备份，在副本变换与校验，再原子提交；失败保留原输入。

0.3.1 以明确版本映射支持 0.1/0.2 模板 1 和 0.3.0/0.3.1 模板 2 的完整重生成校验；未知模板、生成器或内容拒绝。升级不轮换凭据或路径，轮换须显式 init --rotate。保留原二进制、输入与状态用于回退。


## VLESS 与兼容性

新配置默认 `[vless] encryption = true`；旧配置缺少该节时保留旧协议解释，不通过 init 或升级偷偷启用。三种传输的 Encryption 参数分别生成并私密保存，render 和预览不生成或轮换。显式迁移会要求更新客户端。XHTTP TLS 使用 h2,http/1.1，WS 使用 http/1.1；stream-one 不承诺完整 HTTP/1.1 回退。客户端内核和 UDP/QUIC 兼容性以锁定版本验收为准。

## 受保护节点分发（0.4.0 源码，未发布）

新配置包含 `[distribution] enabled = true`，启用 `/nodes/` 页面与订阅。旧配置缺少该节时保持关闭，init 不补齐启用；部署向导明确询问，也可用 `deploy --distribution true|false`。启用时禁止该路径与传输路径或网站 reserved_paths 重叠。

Token 独立保存在 state-dir/distribution/access.json，不进入 node.toml、secrets.json 或配置快照；回滚不恢复已撤销凭据。初始自定义值可在向导无回显输入或用 `--access-token-file`；已有凭据变更必须显式 `share rotate`。使用方式见 [节点访问](node-access.md)。该源码采用模板 3，模板 1/2 和原生成器的严格重生成校验继续保留。
