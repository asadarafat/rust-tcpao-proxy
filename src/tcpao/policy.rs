use std::net::IpAddr;

use crate::config::AoPolicyConfig;

pub fn select_policy(
    policies: &[AoPolicyConfig],
    peer_ip: IpAddr,
    peer_port: Option<u16>,
) -> Option<&AoPolicyConfig> {
    if let Some(port) = peer_port {
        if let Some(exact) = policies
            .iter()
            .find(|policy| policy.peer_ip == peer_ip && policy.peer_port == Some(port))
        {
            return Some(exact);
        }
    }

    if let Some(no_port) = policies
        .iter()
        .find(|policy| policy.peer_ip == peer_ip && policy.peer_port.is_none())
    {
        return Some(no_port);
    }

    if peer_port.is_none() {
        return policies.iter().find(|policy| policy.peer_ip == peer_ip);
    }

    None
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::config::{AoPolicyConfig, KeySource};

    use super::*;

    #[test]
    fn policy_match_with_port_preference() {
        let policies = vec![
            AoPolicyConfig {
                name: "no-port".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: None,
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
            AoPolicyConfig {
                name: "with-port".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: Some(1790),
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
        ];

        let matched = select_policy(
            &policies,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            Some(1790),
        )
        .expect("matching policy");

        assert_eq!(matched.name, "with-port");
    }

    #[test]
    fn policy_falls_back_to_ip_match_when_port_is_missing() {
        let policies = vec![AoPolicyConfig {
            name: "no-port".to_string(),
            peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
            peer_port: None,
            keyid: 1,
            rnextkeyid: None,
            mac_alg: "hmac-sha256".to_string(),
            key_source: KeySource("env:KEY".to_string()),
        }];

        let matched = select_policy(
            &policies,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            Some(1790),
        )
        .expect("matching policy");

        assert_eq!(matched.name, "no-port");
    }

    #[test]
    fn policy_matches_port_specific_entry_when_port_is_unavailable() {
        let policies = vec![AoPolicyConfig {
            name: "with-port".to_string(),
            peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
            peer_port: Some(1790),
            keyid: 1,
            rnextkeyid: None,
            mac_alg: "hmac-sha256".to_string(),
            key_source: KeySource("env:KEY".to_string()),
        }];

        let matched = select_policy(
            &policies,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            None,
        )
        .expect("matching policy");

        assert_eq!(matched.name, "with-port");
    }
}
