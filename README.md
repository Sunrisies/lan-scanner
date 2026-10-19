# lan-scanner

用 Rust 扫描本机局域网中有响应的 IPv4 设备。仅用于你拥有或获准管理的网络。

## 运行

自动识别网段：

```sh
cargo run --release
```

手动指定网段：

```sh
cargo run --release -- 192.168.31.0/24
```

编译发布版本：

```sh
cargo build --release
```

Windows 可执行文件位于 `target\\release\\lan-scanner.exe`；macOS 和 Linux 下位于 `target/release/lan-scanner`。

## 跨平台说明

项目没有第三方 crate 依赖，扫描、并发、TCP 连接和终端进度动画都使用 Rust 标准库。自动识别网段及 ICMP 探测通过操作系统自带命令完成：Windows 使用 PowerShell 和 `ping`，Linux 使用 `ip` 和 `ping`，macOS 使用 `ifconfig` 和 `ping`。若系统没有对应工具或自动识别失败，可手动传入 CIDR 网段。

扫描会并发执行 ICMP ping 和常见 TCP 端口连接。设备若屏蔽 ping 且未开放被探测的端口，可能不会显示。网段支持 `/20` 至 `/30`，最多扫描 4094 个主机地址。
