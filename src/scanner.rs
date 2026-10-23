use crate::network::Network;
use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Device {
    pub ip: Ipv4Addr,
    pub ports: Vec<u16>,
    pub hostname: Option<String>,
}

#[derive(Clone, Copy)]
enum Probe {
    Ping,
    Tcp(u16),
}

impl Probe {
    fn run(self, ip: Ipv4Addr, timeout: Duration) -> bool {
        match self {
            Probe::Ping => ping(ip, timeout),
            Probe::Tcp(port) => {
                TcpStream::connect_timeout(&SocketAddr::from((ip, port)), timeout).is_ok()
            }
        }
    }
}

// A lazy indexed queue avoids allocating one entry for every host/port pair.
struct Jobs {
    hosts: Vec<Ipv4Addr>,
    ports: Vec<u16>,
    next: usize,
}

impl Jobs {
    fn pop(&mut self) -> Option<(Ipv4Addr, Probe)> {
        let per_host = self.ports.len() + 1;
        let total = self.hosts.len().checked_mul(per_host)?;
        if self.next >= total {
            return None;
        }
        let index = self.next;
        self.next += 1;
        let host = self.hosts[index / per_host];
        let probe_index = index % per_host;
        let probe = if probe_index == 0 {
            Probe::Ping
        } else {
            Probe::Tcp(self.ports[probe_index - 1])
        };
        Some((host, probe))
    }
}

fn ping(ip: Ipv4Addr, timeout: Duration) -> bool {
    let address = ip.to_string();
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let ms = timeout.as_millis().clamp(1, 60_000).to_string();
    let mut command = Command::new("ping");
    #[cfg(target_os = "windows")]
    command.args(["-n", "1", "-w", ms.as_str(), address.as_str()]);
    #[cfg(target_os = "macos")]
    command.args(["-n", "-c", "1", "-W", ms.as_str(), address.as_str()]);
    #[cfg(target_os = "linux")]
    {
        let seconds = timeout.as_secs_f64().max(0.001).to_string();
        command.args(["-n", "-c", "1", "-W", seconds.as_str(), address.as_str()]);
    }
    command
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn scan(
    network: Network,
    ports: &[u16],
    timeout: Duration,
    worker_count: usize,
) -> Vec<Device> {
    let hosts: Vec<_> = network.hosts().collect();
    let host_count = hosts.len();
    let probe_count = ports.len() + 1;
    let total_probes = host_count.saturating_mul(probe_count);
    let jobs = Arc::new(Mutex::new(Jobs {
        hosts,
        ports: ports.to_vec(),
        next: 0,
    }));
    let (tx, rx) = mpsc::channel();
    let mut workers = Vec::new();
    for _ in 0..worker_count.min(total_probes).max(1) {
        let jobs = Arc::clone(&jobs);
        let tx = tx.clone();
        workers.push(thread::spawn(move || {
            loop {
                let task = jobs.lock().expect("job queue poisoned").pop();
                let Some((ip, probe)) = task else { break };
                let success = probe.run(ip, timeout);
                let _ = tx.send((ip, probe, success));
            }
        }));
    }
    drop(tx);

    // Count completed probes separately from liveness; all probes can finish in
    // any order because multiple workers process the same host concurrently.
    let mut states: HashMap<Ipv4Addr, (usize, bool, Vec<u16>)> = HashMap::with_capacity(host_count);
    let mut completed_probes = 0usize;
    let mut completed_hosts = 0usize;
    let mut spinner_index = 0usize;
    let spinner = ['|', '/', '-', '\\'];
    let show_animation = std::io::stdout().is_terminal();
    let mut last_render = Instant::now();
    loop {
        let result = rx.recv_timeout(Duration::from_millis(100));
        let (ip, probe, success) = match result {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if show_animation {
                    render_progress(
                        completed_probes,
                        total_probes,
                        states.values().filter(|s| s.1).count(),
                        &spinner,
                        &mut spinner_index,
                    );
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        completed_probes += 1;
        let state = states.entry(ip).or_insert_with(|| (0, false, Vec::new()));
        state.0 += 1;
        match probe {
            Probe::Ping if success => {
                state.1 = true;
                if !show_animation {
                    println!("发现在线设备：{ip}");
                }
            }
            Probe::Tcp(port) if success => {
                state.1 = true;
                state.2.push(port);
                if !show_animation {
                    println!("发现在线设备：{ip}（TCP {port} 开放）");
                }
            }
            _ => {}
        }
        if state.0 == probe_count {
            completed_hosts += 1;
        }
        if show_animation
            && (last_render.elapsed() >= Duration::from_millis(100)
                || completed_probes == total_probes)
        {
            render_progress(
                completed_probes,
                total_probes,
                states.values().filter(|s| s.1).count(),
                &spinner,
                &mut spinner_index,
            );
            last_render = Instant::now();
        }
    }
    if show_animation {
        println!();
    }
    for worker in workers {
        let _ = worker.join();
    }
    let mut found: Vec<_> = states
        .into_iter()
        .filter_map(|(ip, (_, alive, mut ports))| {
            if !alive {
                return None;
            }
            ports.sort_unstable();
            Some(Device {
                ip,
                ports,
                hostname: None,
            })
        })
        .collect();
    found.sort_by_key(|device| u32::from(device.ip));
    eprintln!("探测完成：{completed_hosts}/{host_count} 个地址的所有探测均已结束。");
    found
}

#[cfg(test)]
mod tests {
    use super::{Jobs, Probe};
    use std::net::Ipv4Addr;

    #[test]
    fn lazy_jobs_yield_ping_then_each_port_for_each_host() {
        let first = Ipv4Addr::new(192, 168, 1, 1);
        let second = Ipv4Addr::new(192, 168, 1, 2);
        let mut jobs = Jobs {
            hosts: vec![first, second],
            ports: vec![22, 80],
            next: 0,
        };
        let mut actual = Vec::new();
        while let Some((ip, probe)) = jobs.pop() {
            let name = match probe {
                Probe::Ping => "ping".to_string(),
                Probe::Tcp(port) => port.to_string(),
            };
            actual.push((ip, name));
        }
        assert_eq!(
            actual,
            vec![
                (first, "ping".into()),
                (first, "22".into()),
                (first, "80".into()),
                (second, "ping".into()),
                (second, "22".into()),
                (second, "80".into()),
            ]
        );
        assert!(jobs.pop().is_none());
    }
}

fn render_progress(done: usize, total: usize, found: usize, spinner: &[char], index: &mut usize) {
    let width = 28;
    let filled = done * width / total.max(1);
    let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
    print!(
        "\r{} [{}] {:>3}% 探测 {}/{} | 已发现 {} 台",
        spinner[*index % spinner.len()],
        bar,
        done * 100 / total.max(1),
        done,
        total,
        found
    );
    let _ = std::io::stdout().flush();
    *index += 1;
}
