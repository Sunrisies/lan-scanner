use std::io::Read;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub fn lookup(ip: Ipv4Addr, timeout: Duration) -> Option<String> {
    let timeout_seconds = timeout.as_secs_f64().ceil().max(1.0) as u64;
    let ip_text = ip.to_string();
    let mut child = Command::new("nslookup")
        .args([
            format!("-timeout={timeout_seconds}"),
            format!("-retry=1"),
            ip_text,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout + Duration::from_millis(300);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    for line in output.lines() {
        let line = line.trim();
        let value = line
            .split_once("name = ")
            .map(|(_, name)| name)
            .or_else(|| line.strip_prefix("Name:").map(str::trim));
        if let Some(name) = value {
            let name = name.trim().trim_end_matches('.');
            if !name.is_empty()
                && name.parse::<Ipv4Addr>().is_err()
                && !name.contains("in-addr.arpa")
            {
                return Some(name.to_string());
            }
        }
    }
    None
}
