# Open-source dependencies

所有直接依赖均来自下列 GitHub 开源项目。版本和摘要以 Cargo.lock 为准；许可证来自包元数据，分发时须遵守对应上游许可。

| Library | Version | SPDX license | GitHub |
| --- | --- | --- | --- |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | [source](https://github.com/dtolnay/anyhow) |
| base64 | 0.22.1 | MIT OR Apache-2.0 | [source](https://github.com/marshallpierce/rust-base64) |
| clap | 4.6.7 | MIT OR Apache-2.0 | [source](https://github.com/clap-rs/clap) |
| fs2 | 0.4.3 | MIT/Apache-2.0 | [source](https://github.com/danburkert/fs2-rs) |
| hex | 0.4.3 | MIT OR Apache-2.0 | [source](https://github.com/KokaKiwi/rust-hex) |
| httparse | 1.10.1 | MIT OR Apache-2.0 | [source](https://github.com/seanmonstar/httparse) |
| libc | 0.2.190 | MIT OR Apache-2.0 | [source](https://github.com/rust-lang/libc) |
| minijinja | 2.24.0 | Apache-2.0 | [source](https://github.com/mitsuhiko/minijinja) |
| rand | 0.8.8 | MIT OR Apache-2.0 | [source](https://github.com/rust-random/rand) |
| regex | 1.13.1 | MIT OR Apache-2.0 | [source](https://github.com/rust-lang/regex) |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | [source](https://github.com/servo/rust-url) |
| unicode-general-category | 1.1.0 | Apache-2.0 | [source](https://github.com/yeslogic/unicode-general-category) |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | [source](https://github.com/rustls/rustls) |
| schemars | 0.8.22 | MIT | [source](https://github.com/GREsau/schemars) |
| serde | 1.0.229 | MIT OR Apache-2.0 | [source](https://github.com/serde-rs/serde) |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | [source](https://github.com/serde-rs/json) |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | [source](https://github.com/RustCrypto/hashes) |
| similar | 2.7.0 | Apache-2.0 | [source](https://github.com/mitsuhiko/similar) |
| subtle | 2.6.1 | BSD-3-Clause | [source](https://github.com/dalek-cryptography/subtle) |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | [source](https://github.com/Stebalien/tempfile) |
| toml | 0.8.23 | MIT OR Apache-2.0 | [source](https://github.com/toml-rs/toml) |
| uuid | 1.28.0 | Apache-2.0 OR MIT | [source](https://github.com/uuid-rs/uuid) |
| x25519-dalek | 2.0.1 | BSD-3-Clause | [source](https://github.com/dalek-cryptography/curve25519-dalek/tree/main/x25519-dalek) |

传递依赖由 Cargo.lock 固定，CI 的 cargo-deny 检查许可证和安全公告。当前解析出的许可证均为开源许可；不从私有 registry 或未声明的 Git 来源加载依赖。

外部运行组件： [Xray Core](https://github.com/XTLS/Xray-core)（MPL-2.0）、[Nginx](https://github.com/nginx/nginx)（BSD-2-Clause）、[usque](https://github.com/Diniboy1123/usque)（MIT）、[acme.sh](https://github.com/acmesh-official/acme.sh)（GPL-3.0）。这些组件作为独立进程运行，并未重写或链接进 Rust 程序。

验收客户端另使用 [Mihomo v1.19.32](https://github.com/MetaCubeX/mihomo/tree/v1.19.32)（GPL-3.0）；官方归档摘要固定在 versions.lock.toml。测试下载后作为独立进程执行，不安装到生产服务，也不打包进 nodectl 发行归档。

可选网站服务及 CN 分流另外使用下列独立上游组件。版本、归档与可执行文件或数据文件摘要由 `versions.lock.toml` 固定；安装时从上游获取，不包含在 nodectl 发行归档中。

| Component | Version | Upstream license and source |
| --- | --- | --- |
| LibreSpeed Go（含上游测速界面） | 1.1.6 | [LGPL-3.0 license](https://github.com/librespeed/speedtest-go/blob/v1.1.6/LICENSE), [source](https://github.com/librespeed/speedtest-go/tree/v1.1.6) |
| OpenList | 4.2.7 | [AGPL-3.0 license](https://github.com/OpenListTeam/OpenList/blob/v4.2.7/LICENSE), [source](https://github.com/OpenListTeam/OpenList/tree/v4.2.7) |
| Loyalsoldier GeoIP / GeoSite | 202610172205 | [GPL-3.0 license](https://github.com/Loyalsoldier/v2ray-rules-dat/blob/202610172205/LICENSE), [source and data provenance](https://github.com/Loyalsoldier/v2ray-rules-dat/tree/202610172205) |

项目自有首页只调用独立测速服务，不复制其前端实现；OpenList 保留原生界面和账号权限模型。上游许可证、版权声明和数据来源以各固定版本仓库为准。
