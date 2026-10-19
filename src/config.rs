use crate::network::Network;
use std::time::Duration;

pub const DEFAULT_PORTS: &[u16] = &[22, 53, 80, 443, 445, 554, 8000, 8080, 8443, 9100];

pub struct Config {
    pub network: Network,
    pub ports: Vec<u16>,
    pub timeout: Duration,
    pub workers: usize,
    pub csv_path: Option<String>,
    pub compare_path: Option<String>,
    pub resolve_names: bool,
}

pub fn parse_ports(value: &str) -> Result<Vec<u16>, String> {
    let mut ports = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            let start: u16 = start.parse().map_err(|_| format!("无效端口：{part}"))?;
            let end: u16 = end.parse().map_err(|_| format!("无效端口：{part}"))?;
            if start == 0 || end < start || end - start > 1023 {
                return Err(format!("端口范围无效或超过 1024 个端口：{part}"));
            }
            ports.extend(start..=end);
        } else {
            let port: u16 = part.parse().map_err(|_| format!("无效端口：{part}"))?;
            if port == 0 {
                return Err("端口必须在 1 到 65535 之间".into());
            }
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() {
        return Err("至少指定一个端口".into());
    }
    if ports.len() > 1024 {
        return Err("最多可扫描 1024 个不同端口".into());
    }
    Ok(ports)
}

pub fn usage() -> &'static str {
    "用法：lan-scanner [网段/CIDR] [选项]\n\n\
     示例：lan-scanner\n\
     示例：lan-scanner 192.168.31.0/24 --ports 22,80,443,8000-8010 --timeout-ms 200 --workers 128 --csv scan.csv\n\n\
     选项：\n\
       --ports LIST       端口列表，支持逗号及范围（默认常见 10 个端口）\n\
       --timeout-ms N     每个 TCP 连接等待毫秒数（默认 120）\n\
       --workers N        并发探测数（默认 128，范围 1-512）\n\
       --csv FILE         将结果保存为 CSV\n\
       --compare FILE     与之前导出的 CSV 比较在线设备变化\n\
       --no-hostname      跳过主机名反查\n\
       -h, --help         显示帮助\n\n\
     仅扫描你拥有或获准管理的局域网。没有第三方 crate 依赖。"
}
