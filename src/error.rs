use std::io;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ProxyError>;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("address parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),

    #[error("invalid config: {0}")]
    Config(String),

    #[error("missing mode config section: {0}")]
    MissingModeConfig(&'static str),

    #[error("no AO policy matched peer {0}")]
    NoPolicyForPeer(String),

    #[error("tcp-ao unsupported or not configured: {0}")]
    TcpAo(String),
}
