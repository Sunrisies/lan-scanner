use std::net::Ipv4Addr;
use std::process::Command;

#[derive(Clone, Copy)]
pub struct Network {
    pub base: u32,
    mask: u32,
    pub prefix: u8,
}

impl Network {
    pub fn parse(cidr: &str) -> Result<Self, String> {
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

    pub fn hosts(self) -> impl Iterator<Item = Ipv4Addr> {
        let first = self.base + 1;
        let last = (self.base | !self.mask).saturating_sub(1);
        (first..=last).map(Ipv4Addr::from)
    }
}

pub fn detect_network() -> Result<Network, String> {
    #[cfg(target_os = "windows")]
    let output = Command::new("powershell").args(["-NoProfile", "-Command", "Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | ForEach-Object { \"$($_.IPAddress)/$($_.PrefixLength)\" }"]).output().map_err(|e| format!("无法运行 PowerShell：{e}"))?;
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
    return Err("自动识别网段支持 Windows、macOS 和 Linux；请手动传入 IPv4/CIDR。".into());

    #[cfg(target_os = "windows")]
    {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let candidate = line.trim();
            if let Ok(network) = Network::parse(candidate) {
                let ip = candidate
                    .split('/')
                    .next()
                    .and_then(|s| s.parse::<Ipv4Addr>().ok());
                if ip.is_some_and(|ip| ip.is_private() && !ip.is_loopback() && !ip.is_link_local())
                {
                    return Ok(network);
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let text = String::from_utf8_lossy(&output.stdout);
        // Prefer a private IPv4 address on a typical physical LAN interface over
        // docker, bridge, VPN, and other virtual interfaces when listed first.
        for line in text.lines().filter(|line| !line.contains(" lo ")) {
            let fields: Vec<_> = line.split_whitespace().collect();
            let Some(position) = fields.iter().position(|field| *field == "inet") else {
                continue;
            };
            let Some(candidate) = fields.get(position + 1) else {
                continue;
            };
            if let Ok(network) = Network::parse(candidate) {
                let ip = candidate
                    .split('/')
                    .next()
                    .and_then(|s| s.parse::<Ipv4Addr>().ok());
                if ip.is_some_and(|ip| ip.is_private() && !ip.is_loopback() && !ip.is_link_local())
                {
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
