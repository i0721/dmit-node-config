# 故障排查

| 症状 | 原因、诊断与修复 |
| --- | --- |
| doctor 多项失败 | 按 remedy 先依赖/用户/目录，再证书与端口；诊断不会隐藏后续失败 |
| 安装签名错误 | 核对精确标签、官方 cosign、bundle 与 OIDC 工作流身份；不跳过验证 |
| setup 拒绝覆盖 | 已有输入通过 validate/migrate 管理，不重新生成真实凭据 |
| 证书失败 | 核对仅 DNS、SAN、P-384、信任链、时间和私钥 0600；IP 验证检查公网 80 与所有启用 IPv6 |
| Xray 权限失败 | 核对 xray:www-data 日志目录/文件；不能 root 试读创建 root 专属日志 |
| 端口冲突 | ss -lntup 盘点，保留旧业务，用显式 takeover 接管入口 |
| UDS 不通 | socket 0660，用户 xray、主组 www-data，父目录可访问 |
| H3 不通 | 核对编译模块、UDP 443 与主机/云防火墙；Alt-Svc 不证明公网 QUIC |
| 事务未恢复 | 保留日志和备份，修复外部依赖后 recover，不直接删除 transaction.json/uninstall.json |
| 卸载无基线或有漂移 | 按原始部署记录恢复，工具不能猜测历史状态或覆盖外部修改 |
| WARP 正常但节点直出 | WARP 为独立支路，默认不接管路由，用户客户端无需安装 |

仅分享脱敏版本、诊断项与服务状态，不能上传 UUID、私钥、Token、完整 profiles 或 WARP JSON。见 [详细故障表](deployment.md#13-故障排查)。
