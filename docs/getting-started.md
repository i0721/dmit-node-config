# 首次成功部署

先按 [安装](installation.md) 获得 nodectl。已有业务必须盘点、备份并保留 SSH 与供应商控制台。完整逐步命令见 [部署参考](deployment.md)。

自动交互方式使用 [deploy](automated-deployment.md)，先输出主机差异与 Plan SHA256，经授权再执行。下文保留同一配置模型的手动流程。

1. 准备锁定的 Xray、具备 H2/H3 的 Nginx、可选 usque、OpenSSL、curl 和 systemd。创建 xray/www-data 与日志、网站、证书目录；只添加必要防火墙规则，不重置规则。
2. 预览配置（必须将占位符换成实际值）：

```sh
nodectl setup --non-interactive --domain node.example.com \
  --reality-domain reality.example.com --aliases www.example.com \
  --ipv4 203.0.113.10
```

省略域名/IP 选项时交互输入，`--warp` 显式启用 WARP。审阅后加 `--execute` 原子创建同一套配置与真实随机凭据。已有输入不会被覆盖。

3. 按 [DNS](deployment.md#2-cloudflare-dns-与域名接入) 创建仅 DNS 的 A/可选 AAAA，授权单区域续期 Token。申请 P-384 域名证书，再用临时 HTTP webroot 申请短期 IP 证书。所有启用地址都必须公网可达。
4. 启用 WARP 时单独注册、保护并测试 usque，不接管默认路由；用户客户端无需 WARP。
5. 执行 doctor，修复所有失败项，然后 validate、render、diff、check。参考客户端摘要来自受信任安装记录。已有入口先显式 takeover，再退出临时 HTTP 服务并 apply。
6. apply 成功后从外部验证 H2/H3、裸 IP TLS 和三类握手，保存部署记录。两套证书配置 renew-hook 和 cron，设置 48 小时到期余量检查。

setup 不暗中修改 DNS、申请证书或接管服务；外部步骤逐项预览与审阅。日常命令、备份与失败恢复见 [运维](operations.md)，错误处理见 [故障排查](troubleshooting.md)。
