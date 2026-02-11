#![cfg(target_os = "linux")]

use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use tcpao_proxy::config::{Config, Mode};
use tcpao_proxy::tcpao::linux;

const TEST_KEY_ENV: &str = "TCPAO_TEST_KEY";
const TEST_KEY: &str = "tcpao-functional-key";
const TEST_NO_AO_ENV: &str = "TCPAO_PROXY_TEST_NO_AO";

#[test]
fn simple_traffic_flows_through_two_tcpao_proxies() -> Result<(), Box<dyn Error>> {
    let no_ao_bypass = match linux::probe_tcpao_support() {
        Ok(()) => false,
        Err(err) => {
            if matches!(
                err.kind(),
                io::ErrorKind::Unsupported | io::ErrorKind::PermissionDenied
            ) {
                eprintln!(
                    "tcp-ao unavailable in test host ({err}); enabling {} fallback",
                    TEST_NO_AO_ENV
                );
                true
            } else {
                return Err(Box::new(err));
            }
        }
    };

    let _bypass_guard = if no_ao_bypass {
        ScopedEnvVar::set(TEST_NO_AO_ENV, Some("1"))
    } else {
        ScopedEnvVar::set(TEST_NO_AO_ENV, None)
    };
    let _key_guard = ScopedEnvVar::set(TEST_KEY_ENV, Some(TEST_KEY));

    let temp = TempDir::new()?;

    let initiator_plain_port = free_port()?;
    let terminator_ao_port = free_port()?;
    let terminator_plain_port = free_port()?;

    let initiator_cfg_path = temp.path().join("initiator.toml");
    let terminator_cfg_path = temp.path().join("terminator.toml");

    write_initiator_config(
        &initiator_cfg_path,
        initiator_plain_port,
        terminator_ao_port,
        TEST_KEY_ENV,
    )?;

    write_terminator_config(
        &terminator_cfg_path,
        terminator_ao_port,
        terminator_plain_port,
        TEST_KEY_ENV,
    )?;

    let initiator_cfg = Config::load(&initiator_cfg_path)?;
    initiator_cfg.validate(Mode::Initiator)?;

    let terminator_cfg = Config::load(&terminator_cfg_path)?;
    terminator_cfg.validate(Mode::Terminator)?;

    let echo_addr = SocketAddr::from(([127, 0, 0, 1], terminator_plain_port));
    let echo_listener = TcpListener::bind(echo_addr)?;
    let (echo_done_tx, echo_done_rx) = std::sync::mpsc::channel();

    thread::spawn(move || {
        let result = run_echo_once(echo_listener);
        let _ = echo_done_tx.send(result);
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    let mut terminator_task =
        rt.spawn(async move { tcpao_proxy::mode_terminator::run(terminator_cfg).await });
    let mut initiator_task =
        rt.spawn(async move { tcpao_proxy::mode_initiator::run(initiator_cfg).await });

    let initiator_plain_addr = SocketAddr::from(([127, 0, 0, 1], initiator_plain_port));
    let payload = b"hello-through-tcpao";

    let deadline = Instant::now() + Duration::from_secs(12);
    let mut saw_payload = false;

    while Instant::now() < deadline {
        if initiator_task.is_finished() {
            let outcome = rt.block_on(&mut initiator_task);
            return Err(format!("initiator task exited early: {outcome:?}").into());
        }

        if terminator_task.is_finished() {
            let outcome = rt.block_on(&mut terminator_task);
            return Err(format!("terminator task exited early: {outcome:?}").into());
        }

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

    stop_task(&rt, &mut initiator_task);
    stop_task(&rt, &mut terminator_task);

    if !saw_payload {
        return Err("timed out waiting for end-to-end payload through both proxies".into());
    }

    let echo_result = echo_done_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "echo backend did not finish")?;
    echo_result?;

    Ok(())
}

fn stop_task(rt: &Runtime, task: &mut JoinHandle<Result<(), tcpao_proxy::error::ProxyError>>) {
    task.abort();
    let _ = rt.block_on(task);
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

struct ScopedEnvVar {
    key: &'static str,
    previous: Option<String>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var(key).ok();

        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }

        Self { key, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        if let Some(v) = &self.previous {
            std::env::set_var(self.key, v);
        } else {
            std::env::remove_var(self.key);
        }
    }
}
