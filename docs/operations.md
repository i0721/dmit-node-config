# 运维生命周期

## 状态、诊断与变更

```sh
nodectl status
nodectl status --json
nodectl doctor --json
nodectl validate
nodectl render
# 对生成 ID 执行 diff、check、apply
```

status 汇总当前/上一版本、服务、收据、24 小时证书门槛和中断事务。doctor 聚合依赖、证书、权限、首次端口冲突并给出建议。收据存在不代表刚完成健康检查；上线仍须 check/apply。diff 给出脱敏差异及受影响服务。

## 工具升级及回退

```sh
nodectl upgrade --version v0.2.0
sudo nodectl upgrade --version v0.2.0 --execute
```

升级沿用精确版本签名安装器，先验证已有输入兼容性，失败不替换旧程序。不升级代理依赖、不重启服务。旧二进制按摘要保存在 `/usr/local/lib/nodectl/versions/`，回退先核对摘要，用同目录临时文件与原子替换恢复，再用原版本管理对应快照。

## 卸载与撤销接管

```sh
nodectl uninstall
sudo nodectl uninstall --execute
nodectl recover
```

默认只预览。执行前验证首次基线、备份摘要与当前入口，恢复原文件/链接、启用状态及运行状态。失败恢复安装状态，中断由 recover 撤销卸载；恢复失败保留两个日志并阻止新写入。证书、WARP、用户输入、快照、日志、程序默认保留。

0.1 历史安装没有完整的首次状态记录，升级不能补造事实。缺少 installation.json 时安全拒绝自动卸载，必须根据原始记录恢复入口和原服务状态。不能删除状态目录来绕过检查。

## 证书与告警

首次 apply 成功后为两套 acme.sh 证书配置 `--reloadcmd '/usr/local/bin/nodectl renew-hook'`。webroot 保持公网 80 可达；每 6 小时运行 cron 检查，IP 约三天更新，实际以 CA/ARI 和 notAfter 为准。监控 48 小时余量，不能等到 24 小时上线门槛。

```sh
openssl x509 -in /etc/ssl/dmit/ip/fullchain.pem -noout -checkend 172800
systemctl --failed
journalctl -u nginx -u xray -u usque-warp --since '1 hour ago'
nodectl renew-hook
```

告警接入现有监控接收端，项目没有默认外部联系人。本地失败日志不代表已发送告警。首次续期后检查 serial/notAfter 更新，“未到期跳过”仅证明调度成功。

## 备份、回滚和恢复

私密备份涵盖输入、状态/backups、原工具、双证书、acme.sh、WARP 与摘要。传出主机前加密，隔离环境演练恢复。失败先 recover，再判断 rollback；保留事务和原始证据。完整步骤见 [参考第 11–13 节](deployment.md#11-无人值守续期与监控)。
