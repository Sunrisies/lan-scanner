use crate::network::Network;
use std::collections::{HashMap, VecDeque};
use std::io::{IsTerminal, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

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
        let seconds = (timeout.as_secs_f64().ceil() as u64).max(1).to_string();
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
    let total_probes = host_count * probe_count;
    let mut tasks = VecDeque::with_capacity(total_probes);
    for ip in hosts {
        tasks.push_back((ip, Probe::Ping));
        tasks.extend(ports.iter().copied().map(|port| (ip, Probe::Tcp(port))));
    }

    let jobs = Arc::new(Mutex::new(tasks));
    let (tx, rx) = mpsc::channel();
    let mut workers = Vec::new();
    for _ in 0..worker_count.min(total_probes).max(1) {
        let jobs = Arc::clone(&jobs);
        let tx = tx.clone();
        workers.push(thread::spawn(move || {
            loop {
                let task = jobs.lock().expect("job queue poisoned").pop_front();
                let Some((ip, probe)) = task else {
                    break;
                };
                let success = probe.run(ip, timeout);
                let _ = tx.send((ip, probe, success));
            }
        }));
    }
    drop(tx);

    // done, is_alive, open_ports
    let mut states: HashMap<Ipv4Addr, (usize, bool, Vec<u16>)> = HashMap::with_capacity(host_count);
    let mut found = Vec::new();
    let mut completed_probes = 0usize;
    let mut completed_hosts = 0usize;
    let mut spinner_index = 0usize;
    let spinner = ['|', '/', '-', '\\'];
    let show_animation = std::io::stdout().is_terminal();
    loop {
        let (ip, probe, success) = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if show_animation {
                    let width = 28;
                    let filled = completed_probes * width / total_probes.max(1);
                    let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
                    print!(
                        "\r{} [{}] {:>3}% 探测 {}/{} | 已发现 {} 台",
                        spinner[spinner_index % spinner.len()],
                        bar,
                        completed_probes * 100 / total_probes.max(1),
                        completed_probes,
                        total_probes,
                        found.len()
                    );
                    let _ = std::io::stdout().flush();
                    spinner_index += 1;
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        completed_probes += 1;
        let state = states.entry(ip).or_insert_with(|| (0, false, Vec::new()));
        state.0 += 1;
        match probe {
            Probe::Ping if success => state.1 = true,
            Probe::Tcp(port) if success => {
                state.1 = true;
                state.2.push(port);
            }
            _ => {}
        }
        if state.0 == probe_count {
            completed_hosts += 1;
            if state.1 {
                state.2.sort_unstable();
                found.push(Device {
                    ip,
                    ports: std::mem::take(&mut state.2),
                    hostname: None,
                });
            }
        }
        if show_animation {
            let width = 28;
            let filled = completed_probes * width / total_probes.max(1);
            let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
            print!(
                "\r{} [{}] {:>3}% 探测 {}/{} | 已发现 {} 台",
                spinner[spinner_index % spinner.len()],
                bar,
                completed_probes * 100 / total_probes.max(1),
                completed_probes,
                total_probes,
                found.len()
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
    found.sort_by_key(|device| u32::from(device.ip));
    eprintln!("探测完成：{completed_hosts}/{host_count} 个地址的所有探测均已结束。");
    found
}
