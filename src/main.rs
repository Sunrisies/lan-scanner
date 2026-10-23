mod config;
mod hostname;
mod network;
mod report;
mod scanner;

use config::{COMMON_PORTS, Config, DEFAULT_PORTS, ScanMode, parse_ports, usage};
use network::{Network, detect_network};
use std::env;
use std::time::Duration;

fn parse_config() -> Result<Option<Config>, String> {
    let mut args: std::iter::Peekable<std::vec::IntoIter<String>> = env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .peekable();
    if args.clone().any(|arg| arg == "-h" || arg == "--help") {
        println!("{}", usage());
        return Ok(None);
    }
    let mut cidr = None;
    let mut mode = ScanMode::Fast;
    let mut custom_ports = None;
    let mut timeout_ms = None;
    let mut workers = None;
    let mut csv_path = None;
    let mut compare_path = None;
    let mut resolve_names = true;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-f" => mode = ScanMode::Common,
            "--ports" => {
                custom_ports = Some(parse_ports(&args.next().ok_or("--ports 后需要端口列表")?)?)
            }
            "--timeout-ms" => {
                let value = args
                    .next()
                    .ok_or("--timeout-ms 后需要毫秒数")?
                    .parse()
                    .map_err(|_| "超时必须是整数")?;
                if !(10..=5000).contains(&value) {
                    return Err("超时范围为 10 到 5000 毫秒".into());
                }
                timeout_ms = Some(value);
            }
            "--workers" => {
                let value = args
                    .next()
                    .ok_or("--workers 后需要并发数")?
                    .parse()
                    .map_err(|_| "并发数必须是整数")?;
                if !(1..=512).contains(&value) {
                    return Err("并发数范围为 1 到 512".into());
                }
                workers = Some(value);
            }
            "--csv" => csv_path = Some(args.next().ok_or("--csv 后需要文件路径")?),
            "--compare" => compare_path = Some(args.next().ok_or("--compare 后需要 CSV 文件路径")?),
            "--no-hostname" => resolve_names = false,
            value if value.starts_with('-') => return Err(format!("未知选项：{value}")),
            value => {
                if cidr.replace(value.to_string()).is_some() {
                    return Err("只能指定一个网段".into());
                }
            }
        }
    }
    let network = match cidr {
        Some(cidr) => Network::parse(&cidr)?,
        None => detect_network()?,
    };
    let ports = custom_ports.unwrap_or_else(|| match mode {
        ScanMode::Fast => DEFAULT_PORTS.to_vec(),
        ScanMode::Common => COMMON_PORTS.to_vec(),
    });
    Ok(Some(Config {
        network,
        mode,
        ports,
        timeout: Duration::from_millis(timeout_ms.unwrap_or(120)),
        workers: workers.unwrap_or(128),
        csv_path,
        compare_path,
        resolve_names,
    }))
}

fn run() -> Result<(), String> {
    let Some(config) = parse_config()? else {
        return Ok(());
    };
    let total = (1u64 << (32 - config.network.prefix)) - 2;
    println!(
        "扫描网段：{}/{}，共 {total} 个主机地址",
        std::net::Ipv4Addr::from(config.network.base),
        config.network.prefix
    );
    println!(
        "模式：{} | TCP 端口：{} 个 | 超时：{} ms | 并发：{}",
        config.mode.label(),
        config.ports.len(),
        config.timeout.as_millis(),
        config.workers
    );

    let mut devices = scanner::scan(
        config.network,
        &config.ports,
        config.timeout,
        config.workers,
    );
    if config.resolve_names && !devices.is_empty() {
        println!("正在查询 {} 台设备的反向 DNS 主机名……", devices.len());
        for device in &mut devices {
            device.hostname = hostname::lookup(device.ip, config.timeout);
        }
    }
    println!("\n发现 {} 台有响应的设备：", devices.len());
    for device in &devices {
        let name = device
            .hostname
            .as_deref()
            .map(|n| format!(" ({n})"))
            .unwrap_or_default();
        let ports = if device.ports.is_empty() {
            "无已发现端口".to_string()
        } else {
            format!(
                "开放端口：{}",
                device
                    .ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        println!("{}{name}  {ports}", device.ip);
    }
    if let Some(path) = &config.compare_path {
        report::compare(path, &devices).map_err(|e| format!("读取对比文件 {path} 失败：{e}"))?;
    }
    if let Some(path) = &config.csv_path {
        report::write_csv(path, &devices).map_err(|e| format!("写入 CSV 文件 {path} 失败：{e}"))?;
        println!("\n结果已保存到：{path}");
    }
    println!(
        "\n提示：屏蔽 ping 且未开放所选端口的设备可能不会被发现；反向 DNS 未返回名称时会留空。"
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("错误：{error}\n\n{}", usage());
        std::process::exit(2);
    }
}
