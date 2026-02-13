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
        return policies
            .iter()
            .find(|policy| policy.peer_ip == peer_ip && policy.peer_port.is_none());
    }

    let mut ip_only = policies
        .iter()
        .filter(|policy| policy.peer_ip == peer_ip && policy.peer_port.is_none());
    let ip_only_first = ip_only.next();
    if ip_only_first.is_some() {
        return if ip_only.next().is_none() {
            ip_only_first
        } else {
            None
        };
    }

    let mut any_for_ip = policies.iter().filter(|policy| policy.peer_ip == peer_ip);
    let first = any_for_ip.next();
    if any_for_ip.next().is_none() {
        first
    } else {
        None
    }
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

    #[test]
    fn policy_unknown_port_prefers_ip_only_if_present() {
        let policies = vec![
            AoPolicyConfig {
                name: "with-port".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: Some(1790),
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
            AoPolicyConfig {
                name: "ip-only".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: None,
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
        ];

        let matched = select_policy(
            &policies,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            None,
        )
        .expect("matching policy");

        assert_eq!(matched.name, "ip-only");
    }

    #[test]
    fn policy_unknown_port_fails_when_multiple_port_policies_exist_without_ip_only() {
        let policies = vec![
            AoPolicyConfig {
                name: "with-port-a".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: Some(1790),
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
            AoPolicyConfig {
                name: "with-port-b".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: Some(1791),
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
        ];

        let matched = select_policy(
            &policies,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            None,
        );

        assert!(matched.is_none(), "ambiguous unknown-port match must fail");
    }

    #[test]
    fn policy_order_does_not_change_outcome_for_unknown_port() {
        let forward = vec![
            AoPolicyConfig {
                name: "with-port".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: Some(1790),
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
            AoPolicyConfig {
                name: "ip-only".to_string(),
                peer_ip: IpAddr::from_str("10.0.0.2").expect("valid ip"),
                peer_port: None,
                keyid: 1,
                rnextkeyid: None,
                mac_alg: "hmac-sha256".to_string(),
                key_source: KeySource("env:KEY".to_string()),
            },
        ];
        let reversed = vec![forward[1].clone(), forward[0].clone()];

        let m1 = select_policy(
            &forward,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            None,
        )
        .expect("matching policy");
        let m2 = select_policy(
            &reversed,
            IpAddr::from_str("10.0.0.2").expect("valid ip"),
            None,
        )
        .expect("matching policy");

        assert_eq!(m1.name, m2.name);
        assert_eq!(m1.name, "ip-only");
    }
}
