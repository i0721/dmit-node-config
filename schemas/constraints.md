# 关联与安全约束

JSON Schema 由 `cargo run --locked -- schema` 生成；类型和未知字段由 serde/schemars 处理。`src/config.rs` 另行实施以下规则。

- schema_version 为 1；域名为小写 DNS 名称，IPv4 必填，IPv6 可为空；REALITY 主域与 TLS 主域/别名分开。
- 三个端口均为 1–65535，互不相同。
- 三个 UUID 为不同的规范 UUID，不允许 nil。REALITY 私钥为无填充 base64url 的非零 32 字节；Short ID 为 1–8 字节十六进制。
- 传输路径至少两级，拒绝查询、fragment、编码歧义、点路径、注入字符和协议名称。XHTTP 以斜杠结尾，WS 不以斜杠结尾并精确匹配；各路由不重叠，也不覆盖 reserved_paths 或 ACME 路径。
- 仅支持 stream-one 和 TLSv1.3，必须启用 H2；H3 开启时 Alt-Svc max age 大于 0，关闭时为 0。
- 文件路径是绝对 Linux 路径，只允许字母、数字、下划线、横杠、斜杠和点。拒绝 systemd specifier、Nginx 变量及 shell 插值。UDS 路径不同且小于 100 字节。
- 传输权重非负且总和 100；指纹只允许 chrome/ios/firefox/qq，总权重为 1–1000。
- WARP take_over_default_route 只能为 false，不添加代理出站。
- distribution 为可缺省节；缺省时不启用。启用时 `/nodes/` 不得与传输路由或网站保留路径重叠，分发凭据不属于配置快照。
- 上线额外拒绝文档地址，核实证书、目录、用户访问和实际程序兼容性，并要求真实握手成功。
