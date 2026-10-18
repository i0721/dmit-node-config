# 部署后的节点访问与订阅

本功能随 0.4.0 提供，已通过本地及隔离验收。0.3.1 不包含本页功能；发布不会自动更新现有生产部署。具体范围与限制见 [验收记录](defect/10-secure-distribution-results.md)。

## 打开节点页面

启用分发的部署在健康检查成功后，终端会输出类似结果：

```text
Node page: https://speedtest.example.com/nodes/
Protected access credential: /var/lib/dmit-node/distribution/access.json
Recover access: nodectl share reveal
Rotate: nodectl share rotate
Revoke: nodectl share revoke
```

首次生成的 Token 只在控制终端私密展示。远程自动化没有控制终端时，程序仅输出受保护文件的位置。不要录屏、截图或公开终端会话记录。之后用 `sudo nodectl share reveal` 在私密 SSH 终端取回。

打开节点页面，输入 Token 并点击“验证并打开”。页面展示当前成功版本的节点、权重及导入说明。可以复制 Mihomo / Xray 地址或下载配置；Token 不写入浏览器地址、Cookie 或本地存储，刷新或“清除本页凭据”后重新输入。

![隔离测试中的节点访问页面](assets/distribution/node-access-example.png)

图示为隔离测试示例，通过本地界面适配器访问测试容器；不代表生产部署，不含访问 Token。

复制的订阅地址包含凭据，仅保存到自己的客户端中；不要粘贴到订阅转换站、公共诊断工具、截图或工单。订阅依赖 HTTPS，不提供带凭据的 HTTP 跳转。

## 两种客户端格式

| 格式 | 内容与用法 |
| --- | --- |
| Mihomo | 返回含 proxies 的 provider，可用于自己的 HTTP proxy-providers，再由自己的代理组引用；保留自定义规则 |
| Xray | 返回命名 profiles/outbound 集合，与本地 profiles.json 相同；选用所需 outbound 合并自己的配置，不宣称所有客户端都能直接订阅该集合 |

客户端需要支持当前 VLESS Encryption、Vision 和 XHTTP 设置。节点下载只包含客户端需要的身份、公钥及加密参数，不包含 REALITY 私钥、服务端 decryption 或 WARP 凭据。

Mihomo 的 type、url、path、interval 与代理组引用方式可参照 [官方 proxy-providers 文档](https://wiki.metacubex.one/config/proxy-providers/)。不要把自己的真实订阅地址粘贴进公开示例。

订阅每次获取当前成功部署版本；升级或回滚完成后，在客户端更新订阅即可。部署进行中、恢复未完成、收据不一致或快照被篡改时暂不分发候选，稍后重试。鉴权失败不返回节点内容。

## 轮换、撤销与恢复访问

```sh
sudo nodectl share status
sudo nodectl share rotate
sudo nodectl share reveal
sudo nodectl share revoke
```

rotate 默认生成新的 256 位随机 Token，旧地址随即失效；在自己的客户端替换为新地址。revoke 禁止继续访问，恢复需显式 rotate，回滚不会重新启用旧 Token。若已关闭分发，先重新启用并完成部署，再 rotate；关闭期间不签发可再次启用的访问凭据。

这些写操作与部署串行。存在未恢复事务时，Web 接口暂停分发；先完成 `recover`，再轮换或撤销。正常订阅读取期间，管理命令会短暂等待锁，不能同时改变正在提交的部署。

自定义初始 Token 可在部署向导无回显输入，须为 64 位小写十六进制并包含至少八种字符；应来自密码学随机源。已有 Token 的更改使用 `share rotate --prompt` 或 `share rotate --token-file /root/access-token`，不要把真值放到命令参数中。Token 文件必须归当前操作用户所有，权限 0600。

撤销只阻止今后的页面、下载和订阅访问；已经下载的节点凭据不会被远程删除。若节点身份本身泄露，需要另行显式轮换节点身份并更新自己的客户端。

## 运行与恢复

```sh
sudo nodectl status
sudo systemctl status nodectl-share
sudo nodectl recover
```

分发端点由同一 Rust 程序经 Unix socket 提供，Nginx 负责 HTTPS 和限流，无新增公网端口。启用时相关 Nginx server 关闭请求错误日志，以避免超长请求在路径选择前泄露订阅参数；服务启动配置错误仍可通过 systemd 状态诊断。响应禁用缓存，项目不记录原始 URI 或鉴权头。

配置 `[distribution] enabled = false` 并通过原部署预览及应用流程，可以关闭受保护入口；本地 `export-clients` 仍可使用。完整卸载恢复原服务事实并撤销访问，不删除私密配置和恢复记录。

关闭、卸载或回滚至没有分发功能的旧模板时，在恢复原监听前先撤销订阅凭据，避免原日志记录仍有效的订阅地址。该撤销不因后续失败而逆转；若服务恢复后需要重新开放访问，显式执行 `share rotate` 获取新凭据。节点身份和已下载客户端配置不因此轮换。
