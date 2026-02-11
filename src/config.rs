use std::env;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::error::{ProxyError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Initiator,
    Terminator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub global: GlobalConfig,
    pub initiator: Option<InitiatorConfig>,
    pub terminator: Option<TerminatorConfig>,
    #[serde(default)]
    pub ao_policy: Vec<AoPolicyConfig>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        let cfg = toml::from_str::<Self>(&raw)?;
        Ok(cfg)
    }

    pub fn validate(&self, mode: Mode) -> Result<()> {
        match mode {
            Mode::Initiator => {
                if self.initiator.is_none() {
                    return Err(ProxyError::MissingModeConfig("initiator"));
                }
            }
            Mode::Terminator => {
                if self.terminator.is_none() {
                    return Err(ProxyError::MissingModeConfig("terminator"));
                }
            }
        }

        if self.ao_policy.is_empty() {
            return Err(ProxyError::Config(
                "at least one [[ao_policy]] entry is required".to_string(),
            ));
        }

        for policy in &self.ao_policy {
            let _ = policy.key_source.kind()?;
        }

        Ok(())
    }

    pub fn redacted_summary(&self) -> String {
        format!(
            "log_format={:?}, idle_timeout_secs={}, tcp_keepalive={}, policies={}",
            self.global.log_format,
            self.global.idle_timeout_secs,
            self.global.tcp_keepalive,
            self.ao_policy.len()
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub log_format: LogFormat,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    #[serde(default)]
    pub tcp_keepalive: bool,
    pub keepalive_time_secs: Option<u64>,
    pub keepalive_intvl_secs: Option<u64>,
    pub keepalive_probes: Option<u32>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            log_format: LogFormat::Text,
            idle_timeout_secs: default_idle_timeout_secs(),
            tcp_keepalive: false,
            keepalive_time_secs: None,
            keepalive_intvl_secs: None,
            keepalive_probes: None,
        }
    }
}

impl GlobalConfig {
    pub fn idle_timeout(&self) -> Option<Duration> {
        if self.idle_timeout_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(self.idle_timeout_secs))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitiatorConfig {
    pub listen_plain: String,
    pub remote_ao: String,
}

impl InitiatorConfig {
    pub fn listen_plain_addr(&self) -> Result<SocketAddr> {
        Ok(self.listen_plain.parse()?)
    }

    pub fn remote_ao_addr(&self) -> Result<SocketAddr> {
        Ok(self.remote_ao.parse()?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TerminatorConfig {
    pub listen_ao: String,
    pub forward_plain: String,
}

impl TerminatorConfig {
    pub fn listen_ao_addr(&self) -> Result<SocketAddr> {
        Ok(self.listen_ao.parse()?)
    }

    pub fn forward_plain_addr(&self) -> Result<SocketAddr> {
        Ok(self.forward_plain.parse()?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AoPolicyConfig {
    pub name: String,
    pub peer_ip: IpAddr,
    pub peer_port: Option<u16>,
    pub keyid: u8,
    pub rnextkeyid: Option<u8>,
    pub mac_alg: String,
    pub key_source: KeySource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct KeySource(pub String);

#[derive(Debug, Clone)]
pub enum KeySourceKind {
    File(PathBuf),
    Env(String),
}

impl KeySource {
    pub fn kind(&self) -> Result<KeySourceKind> {
        if let Some(v) = self.0.strip_prefix("file:") {
            let path = PathBuf::from(v);
            if path.as_os_str().is_empty() {
                return Err(ProxyError::Config(
                    "key_source file path must not be empty".to_string(),
                ));
            }
            return Ok(KeySourceKind::File(path));
        }

        if let Some(v) = self.0.strip_prefix("env:") {
            if v.is_empty() {
                return Err(ProxyError::Config(
                    "key_source env variable must not be empty".to_string(),
                ));
            }
            return Ok(KeySourceKind::Env(v.to_string()));
        }

        Err(ProxyError::Config(format!(
            "unsupported key_source '{}'; expected file:PATH or env:VAR",
            self.0
        )))
    }

    pub fn load_key_bytes(&self) -> Result<Vec<u8>> {
        match self.kind()? {
            KeySourceKind::File(path) => {
                let raw = fs::read(path)?;
                if raw.is_empty() {
                    return Err(ProxyError::Config("key file is empty".to_string()));
                }
                Ok(raw)
            }
            KeySourceKind::Env(name) => {
                let raw = env::var(&name).map_err(|_| {
                    ProxyError::Config(format!("required env key '{name}' not found"))
                })?;
                if raw.is_empty() {
                    return Err(ProxyError::Config("env key value is empty".to_string()));
                }
                Ok(raw.into_bytes())
            }
        }
    }
}

fn default_idle_timeout_secs() -> u64 {
    120
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_source_env_is_parsed() {
        let source = KeySource("env:TCPAO_KEY".to_string());
        match source.kind().expect("valid key source") {
            KeySourceKind::Env(name) => assert_eq!(name, "TCPAO_KEY"),
            _ => panic!("unexpected key source kind"),
        }
    }

    #[test]
    fn key_source_rejects_invalid_prefix() {
        let source = KeySource("vault:secret/path".to_string());
        assert!(source.kind().is_err());
    }
}
