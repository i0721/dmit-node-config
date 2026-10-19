# 上游冻结证据

来源为 rin721/console-web 提交 `60189fd7ccbc1c034034ed0ca5ae30d61a448026`。原演示产品的浏览器和 Host 测试以 `.txt` 原样归档；不会运行多用户、mock session 或已移除的演示页面，也不作为 DMIT 当前验收通过的证据。

`tooling/upstream-evidence.json` 登记这些归档和 000–111 原变更记录的 SHA256。根项目文档检查先验证全部摘要，再将原变更内部的旧链接交还上游历史语境；112 及后续记录、当前 authority 和 DMIT 文档仍完整检查。修改冻结内容会使门禁失败，不能借登记表隐藏当前文档。

当前运行测试由 `tooling/foundation-contracts.json` 的现存单元测试和 `apps/web/e2e/dmit.spec.ts`、`protected.spec.ts` 提供。原 `upstreamAuthorityRoutes` 只用于溯源；生产导航只有 DMIT 七页。
