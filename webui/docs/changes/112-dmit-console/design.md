# 设计

## 职责

Web Host 管 fetch、Cookie 会话、CSRF 内存、浏览器下载、路由和主题。Surface 暴露具体 NodeManagementPort 与解析 Schema；DMIT 插件只表达业务语义。Rust 是权限、实际状态和所有副作用的 authority。

保留原 Plugin Catalog/Registry/Next File Route codegen。七个路由位于 /dmit，静态发布 basePath 为 /admin。原演示 identity session 不进入生产。

## 页面模式

- 总览：采样卡、服务异常、发布/事务状态；首个 CPU/速率样本显示等待。
- 节点：真实入口表、订阅风险确认、按需客户端导出；新 Token 仅在显式任务结果中显示一次。
- 配置：环境表单 → 只读基线 → 高级重新认证 → 逐字段及渲染 diff → 检查/完整部署预览 → 输入计划 ID → 二次确认。
- 网络：出口资产、接口、IPv4/IPv6 路由与规则、固定目的地诊断。
- DNS：预期记录、域名/IP 两证书、SAN/日期、只写凭据和受控证书操作。
- 服务：固定托管服务列表和有限操作，未启用项说明原因。
- 运维：任务阶段、恢复异常、健康检查、快照、回滚/恢复、升级和脱敏审计。

组合 Page/PageHeader/Section、Card、DataTable、DescriptionList、ConfirmDialog、Drawer 和 StateSurface。全部值经 API Schema 验证；API 缺失或异常显示错误和恢复路径，不补零、不模拟成功。

## 安全契约

所有 POST 同源 Origin + 会话 CSRF，登录仅要求同源与全局限流。管理员 Token 不放 URL、localStorage/sessionStorage；会话 Cookie HttpOnly/Secure/Strict。输入不发送 Secrets；保护字段解锁由 Rust 决定。原始外部工具输出不直接返回，任务结果有明确成功/失败状态。
