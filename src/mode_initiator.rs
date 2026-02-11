use std::mem;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tracing::{error, info};

use crate::config::{Config, GlobalConfig};
use crate::error::{ProxyError, Result};
use crate::forward::{pump, PumpOptions};
use crate::tcpao::{linux, policy};

static CONN_ID: AtomicU64 = AtomicU64::new(1);

pub async fn run(cfg: Config) -> Result<()> {
    let initiator = cfg
        .initiator
        .as_ref()
        .ok_or(ProxyError::MissingModeConfig("initiator"))?;

    let listen_addr = initiator.listen_plain_addr()?;
    let remote_ao = initiator.remote_ao_addr()?;
    let listener = TcpListener::bind(listen_addr).await?;

    info!(listen = %listen_addr, remote_ao = %remote_ao, "initiator mode listening");

    loop {
        let (plain, plain_peer) = listener.accept().await?;
        let conn_id = CONN_ID.fetch_add(1, Ordering::Relaxed);
        let ao_policies = cfg.ao_policy.clone();
        let global = cfg.global.clone();

        tokio::spawn(async move {
            match handle_connection(conn_id, plain, plain_peer, remote_ao, &ao_policies, &global)
                .await
            {
                Ok(()) => {}
                Err(err) => error!(conn_id, peer = %plain_peer, error = %err, "connection failed"),
            }
        });
    }
}

async fn handle_connection(
    conn_id: u64,
    plain: TcpStream,
    plain_peer: std::net::SocketAddr,
    remote_ao: std::net::SocketAddr,
    policies: &[crate::config::AoPolicyConfig],
    global: &GlobalConfig,
) -> Result<()> {
    let policy = policy::select_policy(policies, remote_ao.ip(), Some(remote_ao.port()))
        .ok_or_else(|| ProxyError::NoPolicyForPeer(remote_ao.to_string()))?;

    let socket = match remote_ao {
        std::net::SocketAddr::V4(_) => TcpSocket::new_v4()?,
        std::net::SocketAddr::V6(_) => TcpSocket::new_v6()?,
    };

    apply_keepalive(socket.as_raw_fd(), global)?;

    linux::apply_outbound_policy(socket.as_raw_fd(), policy, remote_ao)
        .map_err(|e| ProxyError::TcpAo(format!("failed to apply outbound AO policy: {e}")))?;

    let wire = socket.connect(remote_ao).await?;
    apply_keepalive(wire.as_raw_fd(), global)?;
    apply_keepalive(plain.as_raw_fd(), global)?;

    let stats = pump(
        plain,
        wire,
        PumpOptions {
            idle_timeout: global.idle_timeout(),
        },
    )
    .await?;

    info!(
        conn_id,
        peer = %plain_peer,
        policy = %policy.name,
        bytes_up = stats.bytes_up,
        bytes_down = stats.bytes_down,
        duration_ms = stats.duration.as_millis() as u64,
        reason = ?stats.reason,
        "connection closed"
    );

    Ok(())
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
