use std::collections::{HashMap, VecDeque};
use std::env;
use std::io::{IsTerminal, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

const WORKERS: usize = 128;
const TCP_TIMEOUT: Duration = Duration::from_millis(120);
const TCP_PORTS: &[u16] = &[22, 53, 80, 443, 445, 554, 8000, 8080, 8443, 9100];

#[derive(Clone, Copy)]
struct Network {
    base: u32,
    mask: u32,
    prefix: u8,
}

impl Network {
    fn parse(cidr: &str) -> Result<Self, String> {
        let (ip, prefix) = cidr
            .split_once('/')
            .ok_or("网段格式应为 IPv4/前缀，例如 192.168.1.0/24")?;
        let ip: Ipv4Addr = ip.parse().map_err(|_| "无效的 IPv4 地址")?;
        let prefix: u8 = prefix.parse().map_err(|_| "无效的 CIDR 前缀")?;
        if !(20..=30).contains(&prefix) {
            return Err("扫描网段前缀需在 /20 到 /30 之间（最多 4094 个地址）".into());
        }
        let mask = u32::MAX << (32 - prefix);
        Ok(Self {
            base: u32::from(ip) & mask,
            mask,
            prefix,
        })
    }

    fn hosts(self) -> impl Iterator<Item = Ipv4Addr> {
        let first = self.base + 1;
        let last = (self.base | !self.mask).saturating_sub(1);
        (first..=last).map(Ipv4Addr::from)
    }
}

fn detect_network() -> Result<Network, String> {
    #[cfg(target_os = "windows")]
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | ForEach-Object { \"$($_.IPAddress)/$($_.PrefixLength)\" }",
        ])
        .output()
        .map_err(|e| format!("无法运行 PowerShell：{e}"))?;

    #[cfg(target_os = "linux")]
    let output = Command::new("ip")
        .args(["-o", "-4", "addr", "show", "scope", "global"])
        .output()
        .map_err(|e| format!("无法运行 ip 命令：{e}"))?;

    #[cfg(target_os = "macos")]
    let output = Command::new("ifconfig")
        .output()
        .map_err(|e| format!("无法运行 ifconfig：{e}"))?;

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    return Err("自动识别网段目前支持 Windows、macOS 和 Linux；请手动传入 IPv4/CIDR。".into());

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for item in text.split_whitespace() {
            let candidate = item.trim();
            if let Ok(network) = Network::parse(candidate) {
                let ip: Ipv4Addr = candidate.split('/').next().unwrap().parse().unwrap();
                if ip.is_private() {
                    return Ok(network);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with("inet ") || line.contains("127.0.0.1") {
                continue;
            }
            let parts: Vec<_> = line.split_whitespace().collect();
            let (Some(ip_text), Some(mask_text)) = (
                parts.get(1),
                parts
                    .iter()
                    .position(|p| *p == "netmask")
                    .and_then(|i| parts.get(i + 1)),
            ) else {
                continue;
            };
            let Ok(ip) = ip_text.parse::<Ipv4Addr>() else {
                continue;
            };
            let Ok(mask) = u32::from_str_radix(mask_text.trim_start_matches("0x"), 16) else {
                continue;
            };
            let prefix = mask.count_ones() as u8;
            if ip.is_private() && (20..=30).contains(&prefix) {
                return Network::parse(&format!("{ip}/{prefix}"));
            }
        }
    }

    Err("未能自动识别局域网 IPv4 网段。请手动传入网段，例如 192.168.1.0/24".into())
}

fn ping(ip: Ipv4Addr) -> bool {
    let address = ip.to_string();
    #[cfg(target_os = "windows")]
    let args = ["-n", "1", "-w", "800", address.as_str()];
    #[cfg(target_os = "macos")]
    let args = ["-n", "-c", "1", "-W", "800", address.as_str()];
    #[cfg(target_os = "linux")]
    let args = ["-n", "-c", "1", "-W", "1", address.as_str()];

    Command::new("ping")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[derive(Clone, Copy)]
enum Probe {
    Ping,
    Tcp(u16),
}

impl Probe {
    fn run(self, ip: Ipv4Addr) -> bool {
        match self {
            Probe::Ping => ping(ip),
            Probe::Tcp(port) => {
                TcpStream::connect_timeout(&SocketAddr::from((ip, port)), TCP_TIMEOUT).is_ok()
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "用法：lan-scanner [IPv4/CIDR]\n示例：lan-scanner 192.168.1.0/24\n不提供网段时，程序会尝试从本机网络配置自动识别局域网。\n仅扫描你拥有或获准管理的局域网。\n检测方式：ICMP ping + 常见 TCP 端口连接。"
        );
        return;
    }
    let network = match args.first() {
        Some(cidr) => Network::parse(cidr),
        None => detect_network(),
    };
    let network = match network {
        Ok(n) => n,
        Err(e) => {
            eprintln!("错误：{e}\n运行 --help 查看用法。\n");
            std::process::exit(2);
        }
    };
    let total = (1u64 << (32 - network.prefix)) - 2;
    println!(
        "扫描网段：{}/{}，共 {total} 个主机地址（最多 4094）",
        Ipv4Addr::from(network.base),
        network.prefix
    );
    println!(
        "方式：并发 ping 与常见 TCP 端口（{} 个）；结果取决于设备是否响应探测。",
        TCP_PORTS.len()
    );

    let (tx, rx) = mpsc::channel();
    let hosts: Vec<_> = network.hosts().collect();
    let host_count = hosts.len();
    let mut probes = VecDeque::with_capacity(host_count * (TCP_PORTS.len() + 1));
    for ip in hosts {
        probes.push_back((ip, Probe::Ping));
        probes.extend(TCP_PORTS.iter().copied().map(|port| (ip, Probe::Tcp(port))));
    }
    let jobs = Arc::new(Mutex::new(probes));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let jobs = Arc::clone(&jobs);
        let tx = tx.clone();
        workers.push(thread::spawn(move || {
            loop {
                let task = jobs.lock().expect("job queue poisoned").pop_front();
                let Some((ip, probe)) = task else {
                    break;
                };
                let success = probe.run(ip);
                let _ = tx.send((ip, probe, success));
            }
        }));
    }
    drop(tx);

    let mut found = Vec::new();
    let mut states: HashMap<Ipv4Addr, (usize, bool, Vec<u16>)> = HashMap::with_capacity(host_count);
    let mut completed = 0usize;
    let mut spinner_index = 0usize;
    let spinner = ['|', '/', '-', '\\'];
    let show_animation = std::io::stdout().is_terminal();
    while let Ok((ip, probe, success)) = rx.recv_timeout(Duration::from_millis(100)) {
        let state = states.entry(ip).or_insert_with(|| (0, false, Vec::new()));
        state.0 += 1;
        match probe {
            Probe::Ping => state.1 = success,
            Probe::Tcp(port) if success => state.2.push(port),
            Probe::Tcp(_) => {}
        }
        if state.0 == TCP_PORTS.len() + 1 {
            completed += 1;
            if state.1 || !state.2.is_empty() {
                found.push((ip, std::mem::take(&mut state.2)));
            }
        }
        if show_animation {
            let width = 28;
            let filled = completed * width / host_count.max(1);
            let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
            print!(
                "\r{} [{}] {:>3}% ({}/{})",
                spinner[spinner_index % spinner.len()],
                bar,
                completed * 100 / host_count.max(1),
                completed,
                host_count
            );
            let _ = std::io::stdout().flush();
            spinner_index += 1;
        }
    }
    if show_animation {
        println!();
    }
    for worker in workers {
        let _ = worker.join();
    }
    found.sort_by_key(|(ip, _)| u32::from(*ip));
    println!("\n发现 {} 台有响应的设备：", found.len());
    for (ip, ports) in found {
        if ports.is_empty() {
            println!("{ip}");
        } else {
            println!(
                "{ip}  开放端口：{}",
                ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    println!("\n注意：被防火墙屏蔽探测的设备可能不会列出；开放端口也不等于一定可登录。");
}
