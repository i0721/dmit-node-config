# 部署图解

本页使用通用示意图。原控制台截图含特定环境信息，已从公开工作树移除；这些示意不代表实际部署已通过。

## 图 1：A 记录与灰云

区域 example.com 中为 speedtest、files 创建 A 记录，填自己的 IPv4，代理状态“仅 DNS”，TTL 自动。203.0.113.10 仅为示例。见 [DNS 步骤](deployment.md#2-cloudflare-dns-与域名接入)。

## 图 2：可选 IPv6 AAAA 记录

相同名称，类型 AAAA，填可公网访问的真实 IPv6，保持仅 DNS。配置、防火墙、IP 证书 SAN 同步启用，否则全部省略。

## 图 3：DNS Token 模板入口

Cloudflare 个人资料 → API 令牌 → 创建令牌 → 编辑区域 DNS。使用独立续期凭据，不用 Global API Key。

## 图 4：权限与单个 zone 范围

仅授予 example.com 的 DNS 编辑；插件需要查找 zone 时增加区域读取。固定 CF_Zone_ID 且插件支持时可只授予 DNS 编辑。限制实际 API 出口 IP 与有效期，创建前审阅摘要。一次性秘密只保存在 root 专用配置中，不截图。

## 图 5：节点入口与 WARP 的关系

![节点与独立 WARP 支路示意](assets/deployment/network.svg)

443 经 Nginx 与 UDS 进入 Xray，8443 直接进入 REALITY。WARP 不接管默认路由，客户端无需安装 WARP。

## 图 6：两套证书的首次签发与续期

![双证书首次部署与续期示意](assets/deployment/certificate-lifecycle.svg)

先域名证书，再临时 HTTP webroot 与 IP 证书；check 后退出临时服务并 apply；首次成功后启用 renew-hook 与 cron。见 [完整参考](deployment.md)、[运维](operations.md) 和 [记录模板](resources/deployment-record.template.md)。
