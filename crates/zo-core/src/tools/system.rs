use crate::exec;
use std::fs;
use std::process::Command;
use std::time::Duration;

pub fn shell(cmd: &str, timeout: u64) -> Result<String, String> {
    if cmd.trim().is_empty() {
        return Err("No command provided".into());
    }
    exec::run(Command::new("sh").arg("-c").arg(cmd), Duration::from_secs(timeout.clamp(1, 600))).map(|o| o.text())
}

fn kb(line: &str) -> f64 {
    line.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
}

pub fn info() -> Result<String, String> {
    let rd = |p: &str| fs::read_to_string(p).unwrap_or_default().trim().to_string();
    let mut out = vec![
        format!("OS: {} {}", rd("/proc/sys/kernel/ostype"), rd("/proc/sys/kernel/osrelease")),
        format!("Hostname: {}", rd("/proc/sys/kernel/hostname")),
        format!("User: {}", std::env::var("USER").unwrap_or_else(|_| "?".into())),
    ];
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let load = rd("/proc/loadavg");
    out.push(format!("CPU: {cores} cores, load {}", load.split_whitespace().take(3).collect::<Vec<_>>().join(" ")));

    let mem = rd("/proc/meminfo");
    let get = |k: &str| mem.lines().find(|l| l.starts_with(k)).map(kb).unwrap_or(0.0);
    let (total, avail) = (get("MemTotal:"), get("MemAvailable:"));
    if total > 0.0 {
        let gb = 1024.0 * 1024.0;
        out.push(format!("RAM: {:.1} GB total, {:.1} GB used ({:.0}%)", total / gb, (total - avail) / gb, (total - avail) / total * 100.0));
    }
    out.push(match exec::simple("nvidia-smi", &["--query-gpu=name,memory.total", "--format=csv,noheader"]) {
        Ok(s) if !s.contains("[exit code") => format!("GPU: {s}"),
        _ => "GPU: no NVIDIA card found".into(),
    });
    if let Ok(df) = exec::simple("df", &["-h", "--output=source,target,size,used,pcent", "-x", "tmpfs", "-x", "devtmpfs", "-x", "squashfs", "-x", "efivarfs"]) {
        // one line per device: a btrfs system shows the same disk once for every sub-volume
        let mut seen = std::collections::HashSet::new();
        for l in df.lines().skip(1) {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() >= 5 && seen.insert(f[0].to_string()) {
                out.push(format!("Disk {}  {}  used {} ({})", f[1], f[2], f[3], f[4]));
            }
        }
    }
    Ok(out.join("\n"))
}

pub fn processes() -> Result<String, String> {
    let s = exec::simple("ps", &["-eo", "pid,user,pcpu,pmem,comm", "--sort=-pcpu"])?;
    Ok(s.lines().take(41).collect::<Vec<_>>().join("\n"))
}

pub fn kill(pid: u64, force: bool) -> Result<String, String> {
    if pid < 2 {
        return Err("refusing to signal pid 0 or 1".into());
    }
    let sig = if force { "-KILL" } else { "-TERM" };
    exec::simple("kill", &[sig, &pid.to_string()]).map(|o| if o.contains("[exit code") { o } else { format!("Sent {} to {pid}", &sig[1..]) })
}

pub fn screenshot() -> Result<String, String> {
    let dir = crate::audit::data_dir().join("screenshots");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let file = dir.join(format!("screen-{stamp}.png"));
    let f = file.to_string_lossy().into_owned();
    // KDE, then wlroots, then X11
    let tries: [(&str, Vec<&str>); 3] = [
        ("spectacle", vec!["-b", "-n", "-f", "-o", &f]),
        ("grim", vec![&f]),
        ("import", vec!["-window", "root", &f]),
    ];
    for (prog, args) in tries {
        if let Ok(o) = exec::run(Command::new(prog).args(&args), Duration::from_secs(15)) {
            if o.code == Some(0) && file.exists() {
                return Ok(file.to_string_lossy().into_owned());
            }
        }
    }
    Err("no screenshot tool worked (tried spectacle, grim, import)".into())
}

/// Checks that a picture can be shown to a model and returns its full path (the agent attaches the file to the result).
pub fn view_image(path: &str) -> Result<String, String> {
    let full = match path.strip_prefix("~/") {
        Some(rest) => std::env::var("HOME").map(|h| format!("{h}/{rest}")).unwrap_or_else(|_| path.to_string()),
        None => path.to_string(),
    };
    super::guard_read(std::path::Path::new(&full))?;
    if !std::path::Path::new(&full).is_file() {
        return Err(format!("There is no file at {path}."));
    }
    crate::ai::wire::ImageRef::from_path(&full).map(|_| full).ok_or_else(|| "That is not a picture ZO-AI can show (png, jpg, webp or gif, up to about 4 MB).".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_returns_output_and_exit_code() {
        assert_eq!(shell("echo hi", 5).unwrap(), "hi");
        assert!(shell("exit 3", 5).unwrap().contains("[exit code 3]"));
        assert!(shell("sleep 5", 1).is_err(), "a slow command times out");
        assert!(shell("  ", 5).is_err());
    }

    #[test]
    fn it_will_not_kill_init() {
        assert!(kill(1, true).is_err());
    }

    #[test]
    fn system_info_has_the_basics() {
        let s = info().unwrap();
        assert!(s.contains("OS: Linux") && s.contains("RAM:") && s.contains("CPU:"));
    }
}
