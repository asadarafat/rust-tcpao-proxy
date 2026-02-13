use std::mem;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tracing::{error, info};

use crate::config::{AoPolicyConfig, Config, GlobalConfig};
use crate::error::{ProxyError, Result};
use crate::forward::{pump, PumpOptions};
use crate::tcpao::{linux, policy};

static CONN_ID: AtomicU64 = AtomicU64::new(1);
const MODE_LABEL: &str = "terminator";

pub async fn run(cfg: Config) -> Result<()> {
    let terminator = cfg
        .terminator
        .as_ref()
        .ok_or(ProxyError::MissingModeConfig("terminator"))?;

    let listen_addr = terminator.listen_ao_addr()?;
    let forward_plain = terminator.forward_plain_addr()?;
    let listener = build_ao_listener(listen_addr, &cfg.ao_policy)?;

    info!(
        listen = %listen_addr,
        forward_plain = %forward_plain,
        "terminator mode listening"
    );

    loop {
        let (wire, wire_peer) = listener.accept().await?;
        let conn_id = CONN_ID.fetch_add(1, Ordering::Relaxed);
        let ao_policies = cfg.ao_policy.clone();
        let global = cfg.global.clone();

        tokio::spawn(async move {
            match handle_connection(
                conn_id,
                wire,
                wire_peer,
                forward_plain,
                &ao_policies,
                &global,
            )
            .await
            {
                Ok(()) => {}
                Err(err) => {
                    error!(
                        mode = MODE_LABEL,
                        conn_id,
                        peer = %wire_peer,
                        error = %err,
                        "connection failed"
                    )
                }
            }
        });
    }
}

async fn handle_connection(
    conn_id: u64,
    wire: TcpStream,
    wire_peer: std::net::SocketAddr,
    forward_plain: std::net::SocketAddr,
    policies: &[crate::config::AoPolicyConfig],
    global: &GlobalConfig,
) -> Result<()> {
    let policy = policy::select_policy(policies, wire_peer.ip(), None)
        .ok_or_else(|| ProxyError::NoPolicyForPeer(wire_peer.to_string()))?;

    linux::ensure_inbound_session_has_ao(wire.as_raw_fd(), wire_peer)
        .map_err(|e| ProxyError::TcpAo(format!("inbound AO verification failed: {e}")))?;

    let socket = match forward_plain {
        std::net::SocketAddr::V4(_) => TcpSocket::new_v4()?,
        std::net::SocketAddr::V6(_) => TcpSocket::new_v6()?,
    };

    apply_keepalive(socket.as_raw_fd(), global)?;

    let plain = socket.connect(forward_plain).await?;
    apply_keepalive(plain.as_raw_fd(), global)?;
    apply_keepalive(wire.as_raw_fd(), global)?;

    let stats = pump(
        wire,
        plain,
        PumpOptions {
            idle_timeout: global.idle_timeout(),
        },
    )
    .await?;

    info!(
        mode = MODE_LABEL,
        conn_id,
        peer = %wire_peer,
        policy = %policy.name,
        keyid = policy.keyid,
        rnextkeyid = ?policy.rnextkeyid,
        bytes_up = stats.bytes_up,
        bytes_down = stats.bytes_down,
        duration_ms = stats.duration.as_millis() as u64,
        reason = ?stats.reason,
        "connection closed"
    );

    Ok(())
}

fn build_ao_listener(
    listen_addr: std::net::SocketAddr,
    policies: &[AoPolicyConfig],
) -> Result<TcpListener> {
    let domain = match listen_addr {
        std::net::SocketAddr::V4(_) => Domain::IPV4,
        std::net::SocketAddr::V6(_) => Domain::IPV6,
    };

    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&listen_addr.into())?;

    linux::configure_listener(socket.as_raw_fd(), listen_addr, policies)
        .map_err(|e| ProxyError::TcpAo(format!("failed to configure listener AO policies: {e}")))?;

    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    let std_listener: std::net::TcpListener = socket.into();
    Ok(TcpListener::from_std(std_listener)?)
}

#[cfg(target_os = "linux")]
fn apply_keepalive(fd: std::os::fd::RawFd, global: &GlobalConfig) -> Result<()> {
    if !global.tcp_keepalive {
        return Ok(());
    }

    set_sockopt_int(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1)?;

    if let Some(v) = global.keepalive_time_secs {
        set_sockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, v as i32)?;
    }

    if let Some(v) = global.keepalive_intvl_secs {
        set_sockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, v as i32)?;
    }

    if let Some(v) = global.keepalive_probes {
        set_sockopt_int(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, v as i32)?;
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn apply_keepalive(_fd: i32, _global: &GlobalConfig) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_sockopt_int(fd: std::os::fd::RawFd, level: i32, optname: i32, value: i32) -> Result<()> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            optname,
            (&value as *const i32).cast(),
            mem::size_of::<i32>() as libc::socklen_t,
        )
    };

    if rc == 0 {
        return Ok(());
    }

    Err(std::io::Error::last_os_error().into())
}
