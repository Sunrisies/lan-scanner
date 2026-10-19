use crate::scanner::Device;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::net::Ipv4Addr;

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn write_csv(path: &str, devices: &[Device]) -> io::Result<()> {
    let mut text = String::from("ip,hostname,open_ports\n");
    for device in devices {
        let hostname = device.hostname.as_deref().unwrap_or("");
        let ports = device
            .ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(";");
        text.push_str(&format!(
            "{},{},{}\n",
            device.ip,
            csv_escape(hostname),
            csv_escape(&ports)
        ));
    }
    fs::write(path, text)
}

pub fn compare(path: &str, devices: &[Device]) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    let previous: HashSet<Ipv4Addr> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let ip = line.split(',').next()?.trim();
            ip.parse().ok()
        })
        .collect();
    let current: HashSet<Ipv4Addr> = devices.iter().map(|device| device.ip).collect();
    let mut online: Vec<_> = current.difference(&previous).copied().collect();
    let mut offline: Vec<_> = previous.difference(&current).copied().collect();
    online.sort_by_key(|ip| u32::from(*ip));
    offline.sort_by_key(|ip| u32::from(*ip));
    println!(
        "\n与 {path} 比较：新增在线 {} 台，已离线 {} 台。",
        online.len(),
        offline.len()
    );
    if !online.is_empty() {
        println!(
            "新增：{}",
            online
                .iter()
                .map(Ipv4Addr::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !offline.is_empty() {
        println!(
            "离线：{}",
            offline
                .iter()
                .map(Ipv4Addr::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}
