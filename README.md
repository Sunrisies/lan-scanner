# lan-scanner

用 Rust 扫描本机局域网中有响应的 IPv4 设备。仅扫描你拥有或获准管理的网络。

## 运行

自动识别网段：

```sh
cargo run --release
```

指定网段及扫描选项：

```sh
cargo run --release -- 192.168.31.0/24 --ports 22,80,443,8000-8010 --timeout-ms 200 --workers 128 --csv scan.csv
```

查看完整参数：

```sh
cargo run -- --help
```

## 结果保存和比较

第一次保存基准：

```sh
cargo run --release -- --csv previous.csv
```

之后扫描并与基准比较，同时另存新结果：

```sh
cargo run --release -- --compare previous.csv --csv current.csv
```

CSV 包含 IP、反向 DNS 主机名和开放端口。主机名依赖局域网 DNS 的 PTR 记录；没有记录时为空。比较会报告新出现和已离线的 IP。

## 参数

- `--ports 22,80,443,8000-8010`：端口列表和范围；默认扫描 10 个常见端口。
- `--timeout-ms 200`：每个 TCP 端口的连接等待时间，允许 10–5000 毫秒，默认 120 毫秒。
- `--workers 128`：并发探测数，允许 1–512，默认 128。
- `--csv FILE`：将本次结果导出为 CSV。
- `--compare FILE`：与先前导出的 CSV 比较在线设备变化。
- `--no-hostname`：跳过反向 DNS 查询。

扫描采用并发 ICMP ping 和 TCP 连接探测，并显示探测进度和已发现设备数量。设备若屏蔽 ping 且未开放所选端口，可能不会显示。网段支持 `/20` 至 `/30`，最多 4094 个主机地址。

## 跨平台和依赖

项目没有第三方 crate。扫描、并发、TCP 连接、参数处理、CSV 读写和进度动画均使用 Rust 标准库。自动识别网段及 ICMP 探测调用系统自带命令：Windows 使用 PowerShell 和 `ping`，Linux 使用 `ip` 和 `ping`，macOS 使用 `ifconfig` 和 `ping`；反向 DNS 查询使用 `nslookup`。如缺少系统命令或自动识别失败，可手动指定网段。

编译：

```sh
cargo build --release
```

Windows 产物位于 `target\\release\\lan-scanner.exe`；macOS 和 Linux 产物位于 `target/release/lan-scanner`。

## 下载预编译包

发布新版本时，GitHub Actions 会分别构建并发布以下压缩包：

- `lan-scanner-macos-intel.tar.gz`：Intel Mac
- `lan-scanner-macos-apple-silicon.tar.gz`：Apple Silicon Mac（M 系列）
- `lan-scanner-windows-x86_64.zip`：64 位 Windows
- `lan-scanner-linux-x86_64.tar.gz`：64 位 Linux

到 GitHub 仓库的 **Releases** 页面下载与你的系统和处理器匹配的压缩包，解压后按包内 `README.md` 使用。`SHA256SUMS.txt` 可用于核对下载文件完整性。

打版本标签（例如 `v0.1.0`）并推送后，会自动构建这些包并创建 GitHub Release：

```sh
git tag v0.1.0
git push origin v0.1.0
```
