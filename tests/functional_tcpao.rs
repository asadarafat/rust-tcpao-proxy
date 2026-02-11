#![cfg(target_os = "linux")]

use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use tcpao_proxy::tcpao::linux;

const TEST_KEY_ENV: &str = "TCPAO_TEST_KEY";
const TEST_KEY: &str = "tcpao-functional-key";

#[test]
fn simple_traffic_flows_through_two_tcpao_proxies() -> Result<(), Box<dyn Error>> {
    if let Err(err) = linux::probe_tcpao_support() {
        if matches!(
            err.kind(),
            io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
        ) {
            eprintln!("skipping tcp-ao functional test: {err}");
            return Ok(());
        }

        return Err(Box::new(err));
    }

    let temp = TempDir::new()?;

    let initiator_plain_port = free_port()?;
    let terminator_ao_port = free_port()?;
    let terminator_plain_port = free_port()?;

    let initiator_cfg = temp.path().join("initiator.toml");
    let terminator_cfg = temp.path().join("terminator.toml");

    write_initiator_config(
        &initiator_cfg,
        initiator_plain_port,
        terminator_ao_port,
        TEST_KEY_ENV,
    )?;

    write_terminator_config(
        &terminator_cfg,
        terminator_ao_port,
        terminator_plain_port,
        TEST_KEY_ENV,
    )?;

    let echo_addr = SocketAddr::from(([127, 0, 0, 1], terminator_plain_port));
    let echo_listener = TcpListener::bind(echo_addr)?;
    let (echo_done_tx, echo_done_rx) = mpsc::channel();

    thread::spawn(move || {
        let result = run_echo_once(echo_listener);
        let _ = echo_done_tx.send(result);
    });

    let mut terminator = ProxyChild::spawn("terminator", &terminator_cfg, TEST_KEY_ENV, TEST_KEY)?;
    let mut initiator = ProxyChild::spawn("initiator", &initiator_cfg, TEST_KEY_ENV, TEST_KEY)?;

    let initiator_plain_addr = SocketAddr::from(([127, 0, 0, 1], initiator_plain_port));
    let payload = b"hello-through-tcpao";

    let deadline = Instant::now() + Duration::from_secs(12);
    let mut saw_payload = false;

    while Instant::now() < deadline {
        terminator.ensure_running()?;
        initiator.ensure_running()?;

        match TcpStream::connect(initiator_plain_addr) {
            Ok(mut stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;

                if stream.write_all(payload).is_ok() {
                    let mut received = vec![0_u8; payload.len()];
                    if stream.read_exact(&mut received).is_ok() && received == payload {
                        saw_payload = true;
                        break;
                    }
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => return Err(format!("unexpected client connect error: {err}").into()),
        }

        thread::sleep(Duration::from_millis(200));
    }

    if !saw_payload {
        return Err("timed out waiting for end-to-end payload through both proxies".into());
    }

    drop(initiator);
    drop(terminator);

    let echo_result = echo_done_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "echo backend did not finish")?;
    echo_result?;

    Ok(())
}

fn run_echo_once(listener: TcpListener) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    let mut buf = [0_u8; 4096];

    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        stream.write_all(&buf[..n])?;
    }
}

fn free_port() -> io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn write_initiator_config(
    path: &Path,
    initiator_plain_port: u16,
    terminator_ao_port: u16,
    key_env: &str,
) -> io::Result<()> {
    let content = format!(
        "[global]\nlog_format = \"text\"\nidle_timeout_secs = 30\ntcp_keepalive = false\n\n[initiator]\nlisten_plain = \"127.0.0.1:{initiator_plain_port}\"\nremote_ao = \"127.0.0.1:{terminator_ao_port}\"\n\n[[ao_policy]]\nname = \"e2e\"\npeer_ip = \"127.0.0.1\"\nkeyid = 1\nmac_alg = \"hmac-sha1\"\nkey_source = \"env:{key_env}\"\n"
    );

    fs::write(path, content)
}

fn write_terminator_config(
    path: &Path,
    terminator_ao_port: u16,
    terminator_plain_port: u16,
    key_env: &str,
) -> io::Result<()> {
    let content = format!(
        "[global]\nlog_format = \"text\"\nidle_timeout_secs = 30\ntcp_keepalive = false\n\n[terminator]\nlisten_ao = \"127.0.0.1:{terminator_ao_port}\"\nforward_plain = \"127.0.0.1:{terminator_plain_port}\"\n\n[[ao_policy]]\nname = \"e2e\"\npeer_ip = \"127.0.0.1\"\nkeyid = 1\nmac_alg = \"hmac-sha1\"\nkey_source = \"env:{key_env}\"\n"
    );

    fs::write(path, content)
}

struct ProxyChild {
    mode: &'static str,
    child: Child,
}

impl ProxyChild {
    fn spawn(
        mode: &'static str,
        config: &Path,
        key_env: &str,
        key: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let bin = std::env::var("CARGO_BIN_EXE_tcpao-proxy")?;
        let child = Command::new(bin)
            .arg("--mode")
            .arg(mode)
            .arg("--config")
            .arg(config)
            .env(key_env, key)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        Ok(Self { mode, child })
    }

    fn ensure_running(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(status) = self.child.try_wait()? {
            return Err(format!(
                "{0} proxy exited unexpectedly with status {status}",
                self.mode
            )
            .into());
        }

        Ok(())
    }
}

impl Drop for ProxyChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
