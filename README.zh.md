# SSL证书自动续期

Windows 上的 Let's Encrypt DNS-01 证书自动续期工具，提供 Tauri 图形界面、CLI 和受限 DNS 签发代理。

[English README](README.md) · [项目仓库](https://github.com/zengyufei/ssl-renew) · [许可证](LICENSE) · [变更记录](CHANGELOG.md)

## 项目介绍

SSL证书自动续期用于管理多个域名的证书配置，并把证书检查、ACME 订单创建、DNS TXT 记录验证、证书签发保存、Nginx 重启和定时监控串起来。GUI 适合日常配置和手动执行，CLI 适合脚本化，`ssl-signer-agent` 适合把 DNS API Key 放在受限代理进程里，减少主程序直接持有敏感凭据的时间。

本项目适合在 Windows 服务器或运维电脑上管理 Let's Encrypt 证书，尤其适合使用 DNS-01 验证的泛域名证书。它不适合需要商业代码签名、企业级 SLA、跨平台安装器、自动接管所有 DNS/反向代理场景的生产平台；手动 DNS 模式也不适合无人值守续期。

## 截图

下面的截图按主要使用流程排列。`1-5` 对应证书处理步骤，`6` 是监控配置，`7` 是环境变量配置。

<img src="screenshot/1.png" alt="步骤 1：检查证书" width="520">
<img src="screenshot/2.png" alt="步骤 2：创建订单" width="520">
<img src="screenshot/3.png" alt="步骤 3：检测 DNS" width="520">
<img src="screenshot/4.png" alt="步骤 4：签发并保存证书" width="520">
<img src="screenshot/5.png" alt="步骤 5：重启 Nginx" width="520">
<img src="screenshot/6.png" alt="监控配置" width="520">
<img src="screenshot/7.png" alt="环境变量配置" width="520">

## 功能

- 多域名 profile 管理，配置保存在 `profiles.yaml`。
- 支持手动 DNS、Aliyun、Cloudflare 和 signer 代理模式。
- 支持检查证书、创建订单、检测 DNS、签发保存、重启 Nginx 和一键运行。
- 支持定时监控，按天、间隔分钟或 Cron 表达式运行。
- 支持中英文界面、亮色/暗色主题、Toast、日志大小轮转、配置导入导出。
- 支持 UPX 压缩和 GitHub Release zip 打包。

## 环境要求

- Windows x64。
- 运行源码需要 Rust stable、Node.js、npm 和 Tauri 2 依赖。
- 构建压缩版 release 需要仓库内的 `upx-5.1.0-win64/upx.exe`。
- 使用 Nginx 重启功能时，需要本机有 Nginx，并按配置提供 `nginx.exe` 路径和工作目录。

## 安装和运行

从 GitHub Releases 下载 `SSL证书自动续期-vX.Y.Z-windows-x64.zip`，解压后运行 `SSL证书自动续期.exe`。如果 Windows 或安全软件提示未签名 exe，请确认下载来源后再放行；当前项目不提供代码签名。

从源码运行 GUI：

```powershell
cd ssl-renew-gui
npm install
npm run tauri
```

运行 CLI：

```powershell
cargo run -p ssl-renew-cli -- profile list
cargo run -p ssl-renew-cli -- check --domain "*.example.com"
```

运行 signer agent：

```powershell
cargo run -p ssl-signer-agent -- serve
```

## 构建

构建前端：

```powershell
cd ssl-renew-gui
npm run build
```

构建 Rust CLI 和 signer：

```powershell
cargo build --release -p ssl-renew-cli
cargo build --release -p ssl-signer-agent
```

构建 GUI exe：

```powershell
cd ssl-renew-gui
npm run tauri:exe
```

生成压缩后的 release zip：

```powershell
powershell -ExecutionPolicy Bypass -File .\build-release-upx.ps1
```

输出文件位于 `target/release/SSL证书自动续期-vX.Y.Z-windows-x64.zip`。

## 最小使用示例

新增域名配置：

```powershell
target\release\ssl-renew-cli.exe profile add "*.example.com"
target\release\ssl-renew-cli.exe profile set --domain "*.example.com" --email admin@example.com --dns-provider manual --cert-file D:/cert/wildcard.example.com.pem --key-file D:/cert/wildcard.example.com.key
```

手动 DNS 签发流程：

```powershell
target\release\ssl-renew-cli.exe check --domain "*.example.com"
target\release\ssl-renew-cli.exe order --domain "*.example.com" --force
target\release\ssl-renew-cli.exe dns-check --domain "*.example.com"
target\release\ssl-renew-cli.exe issue --domain "*.example.com"
target\release\ssl-renew-cli.exe restart --domain "*.example.com"
```

无人值守续期需要使用 Aliyun、Cloudflare 或 signer 代理，并在配置中启用监控。

## 常见配置

默认配置文件为当前工作目录或上级目录中的 `profiles.yaml`。如果文件不存在，程序会创建带 `*.example.com` 的示例配置。

常见路径默认值：

- 日志文件：`./logs/ssl-renew.log`
- ACME 账号状态：`./state/<domain>/`
- ACME 订单临时数据：`./work/<domain>/`
- 证书文件：`D:/cert/<domain>.pem`
- 私钥文件：`D:/cert/<domain>.key`
- 备份目录：`D:/cert/backup`
- Nginx 程序：`D:/nginx/nginx.exe`
- Nginx 工作目录：`D:/nginx`

“环境变量配置”只管理环境变量名称组，不保存 AccessKey、Token 或任何变量值，也不决定 DNS API 类型。默认提供“阿里云”（`AccessKeyId -> Ali_Key`、`AccessKeySecret -> Ali_Secret`）和 “Cloudflare”（`API Token -> CF_Token`）两组。

多个阿里云账号时，在 Windows 环境变量中分别设置不同变量名，例如 `ALIYUN_A_ID`、`ALIYUN_A_SECRET` 和 `ALIYUN_B_ID`、`ALIYUN_B_SECRET`；然后在 GUI 的“环境变量配置”中新增“阿里云A”“阿里云B”两组，分别填写 `AccessKeyId`、`AccessKeySecret` 别名与对应变量名。在“创建订单”中保留选择 DNS 厂商为 Aliyun，并选择对应环境变量组。执行时程序只显示变量名和是否已设置，并读取所选组的值；绝不会显示或写入变量值。

环境变量组可以加入任意额外变量，执行前都会检查其是否已设置。Aliyun 自动 DNS 仍要求 `AccessKeyId`、`AccessKeySecret` 两个别名，Cloudflare 自动 DNS 仍要求 `API Token` 别名。手动 DNS 和 signer 也可选择组进行预检，但不会消费变量值。不选择环境变量组时，程序继续使用 profile 旧有的 `dns.aliyun.*_env` 或 `dns.cloudflare.api_token_env` 字段，保持兼容。

CLI 也可以管理这些名称组：

```powershell
target\release\ssl-renew-cli.exe env-group add "阿里云A" --id aliyun-a
target\release\ssl-renew-cli.exe env-group add-entry aliyun-a AccessKeyId ALIYUN_A_ID
target\release\ssl-renew-cli.exe env-group add-entry aliyun-a AccessKeySecret ALIYUN_A_SECRET
target\release\ssl-renew-cli.exe profile set --domain "*.example.com" --dns-provider aliyun --env-group aliyun-a
```

## 安全和隐私

本项目会访问文件系统，用于读取和写入 `profiles.yaml`、证书、私钥、日志、ACME 状态和 signer secrets。启用 Nginx 重启时，它会执行本机 Nginx 命令或按配置结束并启动 Nginx 进程。GUI 使用 Tauri 自定义命令完成这些操作，Tauri capability 只启用了默认 core/event/listen 权限。

请不要提交真实的 `profiles.yaml`、`state/`、`work/`、`logs/`、证书、私钥、`.env`、DNS API Key 或 signer secrets。`.gitignore` 已默认排除这些文件。环境变量配置只会保存变量名，不会保存变量值；API Key 和通知 token 属于敏感信息，应只保存在本机受控环境中；signer 的高安全模式会用口令派生密钥加密 DNS Key，并用 Windows DPAPI 保护元数据。

## CI/CD 和发布

GitHub Actions release workflow 通过 `v*` tag 触发。它会在 Windows runner 上安装 Rust 和 Node 依赖，运行核心测试和前端构建，构建三个 exe，执行 UPX 压缩，生成 zip，并上传到 GitHub Releases。

第一版 CI 不执行 `cargo test --workspace`，因为当前 Tauri build script 在 workspace 测试模式下会遇到权限文件路径问题；CI 改为运行 `cargo test -p ssl-core -p ssl-renew-cli -p ssl-signer-agent`。

## 维护状态

这是个人项目，有空才维护，不保证响应时间。欢迎提交 issue 和 PR，但请不要在 issue 中粘贴真实域名账号、API Key、证书私钥或完整运行状态文件。

## 许可证

本项目使用 [MIT License](LICENSE)，允许使用、修改和分发。
