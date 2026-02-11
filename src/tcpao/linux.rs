use std::io;
use std::net::{IpAddr, SocketAddr};

use crate::config::AoPolicyConfig;

#[cfg(target_os = "linux")]
use std::{mem, os::fd::RawFd, ptr};

#[cfg(target_os = "linux")]
use linux_raw_sys::net;
#[cfg(target_os = "linux")]
use tracing::{debug, info};

#[cfg(target_os = "linux")]
const TEST_BYPASS_ENV: &str = "TCPAO_PROXY_TEST_NO_AO";

#[cfg(target_os = "linux")]
pub fn probe_tcpao_support() -> io::Result<()> {
    if allow_test_bypass() {
        info!(
            env = TEST_BYPASS_ENV,
            "tcp-ao test bypass enabled; treating host as supported"
        );
        return Ok(());
    }

    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let result = match get_ao_info(fd) {
        Ok(_) => Ok(()),
        Err(err)
            if err.raw_os_error() == Some(libc::ENOENT)
                || err.kind() == io::ErrorKind::NotFound =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    };

    let _ = unsafe { libc::close(fd) };
    result
}

#[cfg(not(target_os = "linux"))]
pub fn probe_tcpao_support() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tcp-ao is only supported on linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn apply_outbound_policy(
    socket_fd: RawFd,
    policy: &AoPolicyConfig,
    remote: SocketAddr,
) -> io::Result<()> {
    if allow_test_bypass() {
        info!(
            env = TEST_BYPASS_ENV,
            policy = %policy.name,
            peer = %remote,
            "tcp-ao test bypass enabled; skipping outbound ao setsockopt"
        );
        return Ok(());
    }

    let key = policy_key_bytes(policy)?;
    install_key(socket_fd, policy, remote, &key, true)?;
    set_ao_required(socket_fd, true)?;

    info!(
        policy = %policy.name,
        peer = %remote,
        keyid = policy.keyid,
        mac_alg = %policy.mac_alg,
        "applied outbound tcp-ao policy"
    );

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn apply_outbound_policy(
    _socket_fd: i32,
    _policy: &AoPolicyConfig,
    _remote: SocketAddr,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tcp-ao is only supported on linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn configure_listener(
    socket_fd: RawFd,
    listen_addr: SocketAddr,
    policies: &[AoPolicyConfig],
) -> io::Result<()> {
    if allow_test_bypass() {
        info!(
            env = TEST_BYPASS_ENV,
            listen = %listen_addr,
            "tcp-ao test bypass enabled; skipping listener ao setup"
        );
        return Ok(());
    }

    let family = match listen_addr {
        SocketAddr::V4(_) => libc::AF_INET,
        SocketAddr::V6(_) => libc::AF_INET6,
    };

    let mut installed = 0usize;
    for policy in policies {
        if !policy_matches_family(policy.peer_ip, family) {
            continue;
        }

        let peer = SocketAddr::new(policy.peer_ip, policy.peer_port.unwrap_or(0));
        let key = policy_key_bytes(policy)?;
        // At least one listener key must be active for the kernel to authenticate
        // and send AO segments on accepted sessions.
        let set_current = installed == 0;
        install_key(socket_fd, policy, peer, &key, set_current)?;
        installed += 1;
    }

    if installed == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no AO policies matched listener address family",
        ));
    }

    set_ao_required(socket_fd, true)?;

    info!(
        listen = %listen_addr,
        installed,
        "configured tcp-ao policies on listener"
    );

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn configure_listener(
    _socket_fd: i32,
    _listen_addr: SocketAddr,
    _policies: &[AoPolicyConfig],
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tcp-ao is only supported on linux",
    ))
}

#[cfg(target_os = "linux")]
pub fn ensure_inbound_session_has_ao(socket_fd: RawFd, peer: SocketAddr) -> io::Result<()> {
    if allow_test_bypass() {
        debug!(
            env = TEST_BYPASS_ENV,
            peer = %peer,
            "tcp-ao test bypass enabled; skipping inbound ao verification"
        );
        return Ok(());
    }

    let info = match get_ao_info(socket_fd) {
        Ok(info) => info,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            debug!(
                peer = %peer,
                "tcp-ao session info unavailable (ENOENT); continuing with best-effort verification"
            );
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    if info.ao_required() == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "tcp-ao not required on inbound session",
        ));
    }

    debug!(peer = %peer, "verified inbound tcp-ao session state");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn ensure_inbound_session_has_ao(_socket_fd: i32, _peer: SocketAddr) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "tcp-ao is only supported on linux",
    ))
}

#[cfg(target_os = "linux")]
fn policy_matches_family(peer_ip: IpAddr, family: i32) -> bool {
    matches!(
        (peer_ip, family),
        (IpAddr::V4(_), libc::AF_INET) | (IpAddr::V6(_), libc::AF_INET6)
    )
}

#[cfg(target_os = "linux")]
fn allow_test_bypass() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }

    matches!(
        std::env::var(TEST_BYPASS_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

#[cfg(target_os = "linux")]
fn policy_key_bytes(policy: &AoPolicyConfig) -> io::Result<Vec<u8>> {
    policy
        .key_source
        .load_key_bytes()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))
}

#[cfg(target_os = "linux")]
fn install_key(
    socket_fd: RawFd,
    policy: &AoPolicyConfig,
    peer: SocketAddr,
    key: &[u8],
    set_current: bool,
) -> io::Result<()> {
    if key.len() > net::TCP_AO_MAXKEYLEN as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "ao key too long: {} bytes (max {})",
                key.len(),
                net::TCP_AO_MAXKEYLEN
            ),
        ));
    }

    let (alg_name, maclen) = normalize_mac_alg(&policy.mac_alg)?;
    if alg_name.len() >= 64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mac_alg string is too long for kernel tcp_ao_add",
        ));
    }

    let mut add: net::tcp_ao_add = unsafe { mem::zeroed() };
    add.addr = socket_addr_to_kernel_storage(peer);
    add.prefix = prefix_len_for_ip(peer.ip());
    add.sndid = policy.keyid;
    add.rcvid = policy.keyid;
    add.maclen = maclen;
    add.keylen = key.len() as u8;
    add.key[..key.len()].copy_from_slice(key);

    if set_current {
        add.set_set_current(1);
        add.set_set_rnext(1);
    }

    if policy.rnextkeyid.is_some() {
        debug!(
            policy = %policy.name,
            "rnextkeyid configured but rollover semantics are not fully implemented yet"
        );
    }

    for (idx, b) in alg_name.as_bytes().iter().enumerate() {
        add.alg_name[idx] = *b as libc::c_char;
    }

    setsockopt_tcp(
        socket_fd,
        net::TCP_AO_ADD_KEY as i32,
        &add as *const _ as *const libc::c_void,
        mem::size_of::<net::tcp_ao_add>() as libc::socklen_t,
        "TCP_AO_ADD_KEY",
    )
}

#[cfg(target_os = "linux")]
fn normalize_mac_alg(value: &str) -> io::Result<(String, u8)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mac_alg must not be empty",
        ));
    }

    let lower = trimmed.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "hmac-sha1" | "hmac-sha-1" | "hmac(sha1)" => ("hmac(sha1)".to_string(), 12_u8),
        "hmac-sha256" | "hmac-sha-256" | "hmac(sha256)" => ("hmac(sha256)".to_string(), 16_u8),
        "cmac-aes" | "cmac-aes-128" | "cmac(aes)" => ("cmac(aes)".to_string(), 12_u8),
        _ => (trimmed.to_string(), 12_u8),
    };

    Ok(mapped)
}

#[cfg(target_os = "linux")]
fn set_ao_required(socket_fd: RawFd, required: bool) -> io::Result<()> {
    let mut info: net::tcp_ao_info_opt = unsafe { mem::zeroed() };
    info.set_ao_required(u32::from(required));

    setsockopt_tcp(
        socket_fd,
        net::TCP_AO_INFO as i32,
        &info as *const _ as *const libc::c_void,
        mem::size_of::<net::tcp_ao_info_opt>() as libc::socklen_t,
        "TCP_AO_INFO",
    )
}

#[cfg(target_os = "linux")]
fn get_ao_info(socket_fd: RawFd) -> io::Result<net::tcp_ao_info_opt> {
    let mut info: net::tcp_ao_info_opt = unsafe { mem::zeroed() };
    let mut optlen = mem::size_of::<net::tcp_ao_info_opt>() as libc::socklen_t;

    let rc = unsafe {
        libc::getsockopt(
            socket_fd,
            libc::IPPROTO_TCP,
            net::TCP_AO_INFO as i32,
            (&mut info as *mut net::tcp_ao_info_opt).cast(),
            &mut optlen,
        )
    };

    if rc == 0 {
        return Ok(info);
    }

    Err(normalize_ao_error(
        io::Error::last_os_error(),
        "TCP_AO_INFO getsockopt",
    ))
}

#[cfg(target_os = "linux")]
fn setsockopt_tcp(
    socket_fd: RawFd,
    optname: i32,
    optval: *const libc::c_void,
    optlen: libc::socklen_t,
    context: &'static str,
) -> io::Result<()> {
    let rc = unsafe { libc::setsockopt(socket_fd, libc::IPPROTO_TCP, optname, optval, optlen) };
    if rc == 0 {
        return Ok(());
    }

    Err(normalize_ao_error(io::Error::last_os_error(), context))
}

#[cfg(target_os = "linux")]
fn normalize_ao_error(err: io::Error, context: &'static str) -> io::Error {
    match err.raw_os_error() {
        Some(code) if code == libc::ENOPROTOOPT || code == libc::EOPNOTSUPP => io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{context} failed: kernel does not support tcp-ao ({err})"),
        ),
        _ => io::Error::new(err.kind(), format!("{context} failed: {err}")),
    }
}

#[cfg(target_os = "linux")]
fn socket_addr_to_kernel_storage(addr: SocketAddr) -> net::__kernel_sockaddr_storage {
    let storage = socket_addr_to_libc_storage(addr);
    unsafe { mem::transmute(storage) }
}

#[cfg(target_os = "linux")]
fn socket_addr_to_libc_storage(addr: SocketAddr) -> libc::sockaddr_storage {
    let mut storage: libc::sockaddr_storage = unsafe { mem::zeroed() };

    match addr {
        SocketAddr::V4(v4) => {
            let sin = libc::sockaddr_in {
                sin_family: libc::AF_INET as u16,
                sin_port: v4.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(v4.ip().octets()),
                },
                sin_zero: [0; 8],
            };

            unsafe {
                ptr::write(
                    (&mut storage as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in>(),
                    sin,
                )
            };
        }
        SocketAddr::V6(v6) => {
            let sin6 = libc::sockaddr_in6 {
                sin6_family: libc::AF_INET6 as u16,
                sin6_port: v6.port().to_be(),
                sin6_flowinfo: v6.flowinfo(),
                sin6_addr: libc::in6_addr {
                    s6_addr: v6.ip().octets(),
                },
                sin6_scope_id: v6.scope_id(),
            };

            unsafe {
                ptr::write(
                    (&mut storage as *mut libc::sockaddr_storage).cast::<libc::sockaddr_in6>(),
                    sin6,
                )
            };
        }
    }

    storage
}

#[cfg(target_os = "linux")]
fn prefix_len_for_ip(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_unspecified() {
                0
            } else {
                32
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                0
            } else {
                128
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn normalize_mac_alg_maps_known_values() {
        let (name, maclen) = normalize_mac_alg("hmac-sha1").expect("valid alg");
        assert_eq!(name, "hmac(sha1)");
        assert_eq!(maclen, 12);

        let (name, maclen) = normalize_mac_alg("hmac-sha256").expect("valid alg");
        assert_eq!(name, "hmac(sha256)");
        assert_eq!(maclen, 16);
    }

    #[test]
    fn prefix_len_uses_full_prefix_for_specific_addresses() {
        assert_eq!(
            prefix_len_for_ip("127.0.0.1".parse().expect("valid ip")),
            32
        );
        assert_eq!(prefix_len_for_ip("::1".parse().expect("valid ip")), 128);
    }

    #[test]
    fn prefix_len_allows_unspecified_wildcard() {
        assert_eq!(prefix_len_for_ip("0.0.0.0".parse().expect("valid ip")), 0);
        assert_eq!(prefix_len_for_ip("::".parse().expect("valid ip")), 0);
    }
}
