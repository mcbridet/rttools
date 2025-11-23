use anyhow::{Context, Result};
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

/// Watches kernel log output and mirrors lines mentioning a given tape device.
pub struct KernelLogWatcher {
    child: Child,
    handle: Option<thread::JoinHandle<()>>,
}

impl KernelLogWatcher {
    pub fn start(device_tokens: Vec<String>) -> Result<Self> {
        if device_tokens.is_empty() {
            anyhow::bail!("no device tokens provided for kernel log capture");
        }

        let normalized_tokens: Vec<String> = device_tokens
            .into_iter()
            .map(|token| token.to_lowercase())
            .collect();

        let (mut child, source_label) = spawn_log_source()?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture kernel log stdout")?;

        let handle = thread::Builder::new()
            .name("kernel-log".into())
            .spawn(move || {
                pump_kernel_output(stdout, normalized_tokens, source_label);
            })
            .context("failed to start kernel log reader thread")?;

        Ok(Self {
            child,
            handle: Some(handle),
        })
    }
}

impl Drop for KernelLogWatcher {
    fn drop(&mut self) {
        // Kill follow process so the reader thread can exit promptly.
        let _ = self.child.kill();
        let _ = self.child.wait();

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn spawn_log_source() -> Result<(Child, &'static str)> {
    match spawn_journalctl() {
        Ok(child) => Ok((child, "journalctl")),
        Err(journal_err) => match spawn_dmesg() {
            Ok(child) => Ok((child, "dmesg")),
            Err(dmesg_err) => Err(anyhow::anyhow!(
                "failed to start journalctl ({journal_err}); failed to start dmesg ({dmesg_err})"
            )),
        },
    }
}

fn spawn_journalctl() -> Result<Child> {
    let mut child = Command::new("journalctl")
        .args(["-kf", "-n0", "--no-pager", "-o", "short"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("unable to spawn journalctl")?;

    ensure_long_running(&mut child, "journalctl")?;
    Ok(child)
}

fn spawn_dmesg() -> Result<Child> {
    let mut child = Command::new("dmesg")
        .args([
            "--follow",
            "--since=now",
            "--color=never",
            "--kernel",
            "--notime",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("unable to spawn dmesg")?;

    ensure_long_running(&mut child, "dmesg")?;
    Ok(child)
}

fn ensure_long_running(child: &mut Child, label: &str) -> Result<()> {
    thread::sleep(Duration::from_millis(50));
    if let Some(status) = child
        .try_wait()
        .with_context(|| format!("failed to poll {label} status"))?
    {
        anyhow::bail!("{label} exited immediately with status {status}");
    }
    Ok(())
}

fn pump_kernel_output(mut stdout: ChildStdout, tokens: Vec<String>, label: &'static str) {
    let mut reader = BufReader::new(&mut stdout);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if should_emit(trimmed, &tokens) {
                    eprintln!("[kernel:{label}] {trimmed}");
                }
            }
            Err(err) => {
                eprintln!("[kernel:{label}] error reading kernel log: {err}");
                break;
            }
        }
    }
}

fn should_emit(line: &str, tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return true;
    }

    let lower = line.to_lowercase();
    tokens.iter().any(|needle| lower.contains(needle))
}
