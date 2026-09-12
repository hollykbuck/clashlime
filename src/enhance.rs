use anyhow::{Context, Result, bail};
use serde_yaml_ng::{Mapping, Value};
use std::{fs, path::Path};

const SEQUENCES: [(&str, &str); 3] = [
    ("rules", "rules"),
    ("proxies", "proxies"),
    ("proxy-groups", "proxy-groups"),
];

pub fn build_runtime(
    base: &Path,
    merge: Option<&Path>,
    chains: &[(&str, &Path)],
) -> Result<Mapping> {
    let mut config = read_mapping(base)?;
    if let Some(path) = merge.filter(|path| path.exists()) {
        let patch = read_mapping(path)?;
        apply_merge(&mut config, patch);
    }
    for (key, path) in chains.iter().filter(|(_, path)| path.exists()) {
        let value: Value = serde_yaml_ng::from_str(&fs::read_to_string(path)?)
            .with_context(|| format!("invalid enhancement {}", path.display()))?;
        let sequence = match value {
            Value::Sequence(sequence) => sequence,
            Value::Mapping(mapping) => mapping
                .get(*key)
                .and_then(Value::as_sequence)
                .cloned()
                .unwrap_or_default(),
            _ => bail!("{} must contain a YAML sequence", path.display()),
        };
        config.insert(Value::String((*key).into()), Value::Sequence(sequence));
    }
    Ok(config)
}

pub fn apply_runtime_defaults(config: &mut Mapping, cfg: &crate::config::Config) {
    let controller = cfg
        .controller
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    set(config, "external-controller", controller);
    set(config, "secret", cfg.secret.clone());
    set(config, "mixed-port", cfg.mixed_port);
    set(config, "allow-lan", cfg.allow_lan);
    set(config, "ipv6", cfg.ipv6);
    if let Some(port) = cfg.socks_port {
        set(config, "socks-port", u64::from(port));
    }
    if let Some(port) = cfg.http_port {
        set(config, "port", u64::from(port));
    }
    if let Some(port) = cfg.redir_port {
        set(config, "redir-port", u64::from(port));
    }
    if let Some(port) = cfg.tproxy_port {
        set(config, "tproxy-port", u64::from(port));
    }
    if !cfg.authentication.is_empty() {
        set(
            config,
            "authentication",
            cfg.authentication
                .iter()
                .map(|s| Value::String(s.clone().into()))
                .collect::<Vec<_>>(),
        );
    }
    if !cfg.skip_auth_prefixes.is_empty() {
        set(
            config,
            "skip-auth-prefixes",
            cfg.skip_auth_prefixes
                .iter()
                .map(|s| Value::String(s.clone().into()))
                .collect::<Vec<_>>(),
        );
    }
    if !cfg.lan_allowed_ips.is_empty() {
        set(
            config,
            "lan-allowed-ips",
            cfg.lan_allowed_ips
                .iter()
                .map(|s| Value::String(s.clone().into()))
                .collect::<Vec<_>>(),
        );
    }
    if !cfg.lan_disallowed_ips.is_empty() {
        set(
            config,
            "lan-disallowed-ips",
            cfg.lan_disallowed_ips
                .iter()
                .map(|s| Value::String(s.clone().into()))
                .collect::<Vec<_>>(),
        );
    }
    if let Some(concurrent) = cfg.tcp_concurrent {
        set(config, "tcp-concurrent", concurrent);
    }
    if let Some(unified) = cfg.unified_delay {
        set(config, "unified-delay", unified);
    }
    if let Some(mode) = &cfg.find_process_mode {
        set(config, "find-process-mode", mode.clone());
    }
    apply_tun_config(config, &cfg.tun);
    let profile = config
        .entry(Value::String("profile".into()))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if let Value::Mapping(mapping) = profile {
        mapping
            .entry(Value::String("store-selected".into()))
            .or_insert(Value::Bool(true));
    }
}

pub fn apply_dns_config(config: &mut Mapping, dns: &crate::config::DnsConfig) {
    if !dns.enable {
        return;
    }
    let mut dns_map = Mapping::new();
    dns_map.insert(Value::String("enable".into()), Value::Bool(true));
    dns_map.insert(
        Value::String("listen".into()),
        Value::String(dns.listen.clone().into()),
    );
    dns_map.insert(Value::String("ipv6".into()), Value::Bool(dns.ipv6));
    if let Some(mode) = &dns.enhanced_mode {
        dns_map.insert(
            Value::String("enhanced-mode".into()),
            Value::String(mode.clone().into()),
        );
    }
    if let Some(range) = &dns.fake_ip_range {
        dns_map.insert(
            Value::String("fake-ip-range".into()),
            Value::String(range.clone().into()),
        );
    }
    if !dns.nameserver.is_empty() {
        let seq = dns
            .nameserver
            .iter()
            .map(|s| Value::String(s.clone().into()))
            .collect();
        dns_map.insert(Value::String("nameserver".into()), Value::Sequence(seq));
    }
    if !dns.fallback.is_empty() {
        let seq = dns
            .fallback
            .iter()
            .map(|s| Value::String(s.clone().into()))
            .collect();
        dns_map.insert(Value::String("fallback".into()), Value::Sequence(seq));
    }
    let string_list = |items: &[String]| {
        Value::Sequence(
            items
                .iter()
                .map(|s| Value::String(s.clone().into()))
                .collect(),
        )
    };
    if let Some(mode) = &dns.fake_ip_filter_mode {
        dns_map.insert(
            Value::String("fake-ip-filter-mode".into()),
            Value::String(mode.clone().into()),
        );
    }
    if !dns.fake_ip_filter.is_empty() {
        dns_map.insert(
            Value::String("fake-ip-filter".into()),
            string_list(&dns.fake_ip_filter),
        );
    }
    if let Some(respect) = dns.respect_rules {
        dns_map.insert(
            Value::String("respect-rules".into()),
            Value::Bool(respect),
        );
    }
    if !dns.default_nameserver.is_empty() {
        dns_map.insert(
            Value::String("default-nameserver".into()),
            string_list(&dns.default_nameserver),
        );
    }
    if !dns.proxy_server_nameserver.is_empty() {
        dns_map.insert(
            Value::String("proxy-server-nameserver".into()),
            string_list(&dns.proxy_server_nameserver),
        );
    }
    if !dns.direct_nameserver.is_empty() {
        dns_map.insert(
            Value::String("direct-nameserver".into()),
            string_list(&dns.direct_nameserver),
        );
    }
    if !dns.nameserver_policy.is_empty() {
        let mut policy = Mapping::new();
        for (domain, server) in &dns.nameserver_policy {
            policy.insert(
                Value::String(domain.clone().into()),
                Value::String(server.clone().into()),
            );
        }
        dns_map.insert(
            Value::String("nameserver-policy".into()),
            Value::Mapping(policy),
        );
    }
    if !dns.hosts.is_empty() {
        let mut hosts = Mapping::new();
        for (domain, value) in &dns.hosts {
            hosts.insert(
                Value::String(domain.clone().into()),
                Value::String(value.clone().into()),
            );
        }
        dns_map.insert(Value::String("hosts".into()), Value::Mapping(hosts));
    }
    if let Some(use_system_hosts) = dns.use_system_hosts {
        dns_map.insert(
            Value::String("use-system-hosts".into()),
            Value::Bool(use_system_hosts),
        );
    }
    if !dns.fallback_filter.is_empty() {
        let mut filter = Mapping::new();
        if let Some(geoip) = dns.fallback_filter.geoip {
            filter.insert(Value::String("geoip".into()), Value::Bool(geoip));
        }
        if let Some(code) = &dns.fallback_filter.geoip_code {
            filter.insert(
                Value::String("geoip-code".into()),
                Value::String(code.clone().into()),
            );
        }
        if !dns.fallback_filter.ipcidr.is_empty() {
            filter.insert(
                Value::String("ipcidr".into()),
                string_list(&dns.fallback_filter.ipcidr),
            );
        }
        if !dns.fallback_filter.domain.is_empty() {
            filter.insert(
                Value::String("domain".into()),
                string_list(&dns.fallback_filter.domain),
            );
        }
        dns_map.insert(
            Value::String("fallback-filter".into()),
            Value::Mapping(filter),
        );
    }
    // Preserve profile-provided dns keys we don't manage via deep merge
    if let Some(Value::Mapping(existing)) = config.get(&Value::String("dns".into())).cloned() {
        let mut merged = existing;
        deep_merge(&mut merged, dns_map);
        config.insert(Value::String("dns".into()), Value::Mapping(merged));
    } else {
        config.insert(Value::String("dns".into()), Value::Mapping(dns_map));
    }
}

/// Inject the TUN section when enabled; otherwise strip any
/// profile-provided `tun` as before (TUN needs root and surprises users).
fn apply_tun_config(config: &mut Mapping, tun: &crate::config::TunConfig) {
    if !tun.enable {
        config.remove("tun");
        return;
    }
    let mut map = Mapping::new();
    map.insert(Value::String("enable".into()), Value::Bool(true));
    if let Some(stack) = &tun.stack {
        map.insert(
            Value::String("stack".into()),
            Value::String(stack.clone().into()),
        );
    }
    if let Some(device) = &tun.device {
        map.insert(
            Value::String("device".into()),
            Value::String(device.clone().into()),
        );
    }
    if let Some(auto_route) = tun.auto_route {
        map.insert(
            Value::String("auto-route".into()),
            Value::Bool(auto_route),
        );
    }
    if let Some(auto_detect) = tun.auto_detect_interface {
        map.insert(
            Value::String("auto-detect-interface".into()),
            Value::Bool(auto_detect),
        );
    }
    if let Some(strict) = tun.strict_route {
        map.insert(
            Value::String("strict-route".into()),
            Value::Bool(strict),
        );
    }
    if let Some(redirect) = tun.auto_redirect {
        map.insert(
            Value::String("auto-redirect".into()),
            Value::Bool(redirect),
        );
    }
    if !tun.route_exclude_address.is_empty() {
        map.insert(
            Value::String("route-exclude-address".into()),
            Value::Sequence(
                tun.route_exclude_address
                    .iter()
                    .map(|s| Value::String(s.clone().into()))
                    .collect(),
            ),
        );
    }
    if !tun.dns_hijack.is_empty() {
        map.insert(
            Value::String("dns-hijack".into()),
            Value::Sequence(
                tun.dns_hijack
                    .iter()
                    .map(|s| Value::String(s.clone().into()))
                    .collect(),
            ),
        );
    }
    if let Some(mtu) = tun.mtu {
        map.insert(
            Value::String("mtu".into()),
            Value::Number(u64::from(mtu).into()),
        );
    }
    config.insert(Value::String("tun".into()), Value::Mapping(map));
}

/// Inject a minimal sniffer override; profile-provided sniffer keys are
/// preserved via deep merge, mirroring the DNS override behavior.
pub fn apply_sniffer_config(
    config: &mut Mapping,
    enabled: bool,
    sniffer: &crate::config::SnifferConfig,
) {
    if !enabled {
        return;
    }
    let mut map = Mapping::new();
    map.insert(Value::String("enable".into()), Value::Bool(true));
    if let Some(force) = sniffer.force_dns_mapping {
        map.insert(Value::String("force-dns-mapping".into()), Value::Bool(force));
    }
    if let Some(parse) = sniffer.parse_pure_ip {
        map.insert(Value::String("parse-pure-ip".into()), Value::Bool(parse));
    }
    if let Some(override_dest) = sniffer.override_destination {
        map.insert(
            Value::String("override-destination".into()),
            Value::Bool(override_dest),
        );
    }
    let mut sniff = Mapping::new();
    if !sniffer.http_ports.is_empty() {
        let mut http = Mapping::new();
        http.insert(
            Value::String("ports".into()),
            Value::Sequence(sniffer.http_ports.iter().map(sniff_port_value).collect()),
        );
        sniff.insert(Value::String("HTTP".into()), Value::Mapping(http));
    }
    if !sniffer.tls_ports.is_empty() {
        let mut tls = Mapping::new();
        tls.insert(
            Value::String("ports".into()),
            Value::Sequence(sniffer.tls_ports.iter().map(sniff_port_value).collect()),
        );
        sniff.insert(Value::String("TLS".into()), Value::Mapping(tls));
    }
    if !sniff.is_empty() {
        map.insert(Value::String("sniff".into()), Value::Mapping(sniff));
    }
    if let Some(Value::Mapping(existing)) = config
        .get(&Value::String("sniffer".into()))
        .cloned()
    {
        let mut merged = existing;
        deep_merge(&mut merged, map);
        config.insert(Value::String("sniffer".into()), Value::Mapping(merged));
    } else {
        config.insert(Value::String("sniffer".into()), Value::Mapping(map));
    }
}

/// Sniff port entries: bare ports become numbers, `start-end` ranges stay
/// strings (entries are validated in the TUI before they get here).
fn sniff_port_value(entry: &String) -> Value {
    match entry.parse::<u64>() {
        Ok(port) => Value::Number(port.into()),
        Err(_) => Value::String(entry.clone().into()),
    }
}

/// Inject geo database settings so the running core keeps them fresh
/// itself (mirrors clash-party's controled `geox-url` / `geo-auto-update`).
/// URLs honor the configured mirror; validation still uses the files in the
/// data dir ensured by [`crate::geo::ensure_for_content`].
pub fn apply_geo_config(config: &mut Mapping, geo: &crate::config::GeoConfig) {
    let mirror = crate::geo::effective_mirror(geo.mirror.as_deref());
    let url = |asset: &str, override_url: Option<&str>| match override_url
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        Some(custom) => custom.to_owned(),
        None => match mirror.as_deref() {
            Some(m) => format!("{}/{asset}", m.trim_end_matches('/')),
            None => asset.to_owned(),
        },
    };
    let mut geox = Mapping::new();
    geox.insert(
        Value::String("geoip".into()),
        Value::String(
            url(
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip-lite.dat",
                geo.geoip_url.as_deref(),
            )
            .into(),
        ),
    );
    geox.insert(
        Value::String("geosite".into()),
        Value::String(
            url(
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat",
                geo.geosite_url.as_deref(),
            )
            .into(),
        ),
    );
    geox.insert(
        Value::String("mmdb".into()),
        Value::String(
            url(
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb",
                geo.mmdb_url.as_deref(),
            )
            .into(),
        ),
    );
    geox.insert(
        Value::String("asn".into()),
        Value::String(
            url(
                "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/GeoLite2-ASN.mmdb",
                geo.asn_url.as_deref(),
            )
            .into(),
        ),
    );
    config
        .entry(Value::String("geodata-mode".into()))
        .or_insert(Value::Bool(false));
    config
        .entry(Value::String("geox-url".into()))
        .or_insert(Value::Mapping(geox));
    if geo.auto_update {
        config
            .entry(Value::String("geo-auto-update".into()))
            .or_insert(Value::Bool(true));
        config
            .entry(Value::String("geo-update-interval".into()))
            .or_insert(Value::Number(geo.update_interval.into()));
    }
}

fn apply_merge(config: &mut Mapping, mut patch: Mapping) {
    for (name, target) in SEQUENCES {
        let prepend = patch.remove(Value::String(format!("prepend-{name}")));
        let append = patch.remove(Value::String(format!("append-{name}")));
        if prepend.is_some() || append.is_some() {
            let existing = config
                .get(target)
                .and_then(Value::as_sequence)
                .cloned()
                .unwrap_or_default();
            let mut combined = prepend
                .and_then(|v| v.as_sequence().cloned())
                .unwrap_or_default();
            combined.extend(existing);
            combined.extend(
                append
                    .and_then(|v| v.as_sequence().cloned())
                    .unwrap_or_default(),
            );
            patch.insert(Value::String(target.into()), Value::Sequence(combined));
        }
    }
    deep_merge(config, patch);
}

fn deep_merge(target: &mut Mapping, patch: Mapping) {
    for (key, value) in patch {
        match (target.get_mut(&key), value) {
            (Some(Value::Mapping(existing)), Value::Mapping(incoming)) => {
                deep_merge(existing, incoming)
            }
            (_, value) => {
                target.insert(key, value);
            }
        }
    }
}

fn read_mapping(path: &Path) -> Result<Mapping> {
    serde_yaml_ng::from_str(
        &fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("invalid YAML in {}", path.display()))
}

fn set(mapping: &mut Mapping, key: &str, value: impl Into<Value>) {
    mapping.insert(Value::String(key.into()), value.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merge_supports_prepend_and_append() {
        let mut base: Mapping =
            serde_yaml_ng::from_str("rules: [base]\ndns: {enable: false}\n").unwrap();
        let patch: Mapping = serde_yaml_ng::from_str(
            "prepend-rules: [first]\nappend-rules: [last]\ndns: {enable: true}\n",
        )
        .unwrap();
        apply_merge(&mut base, patch);
        assert_eq!(base["rules"].as_sequence().unwrap().len(), 3);
        assert_eq!(base["dns"]["enable"], Value::Bool(true));
    }

    #[test]
    fn geox_url_honors_per_asset_overrides() {
        let mut geo = crate::config::GeoConfig::default();
        geo.mirror = Some("https://gh-proxy.com".into());
        geo.mmdb_url = Some("https://cdn.example.com/geoip.metadb".into());
        let mut config = Mapping::new();
        apply_geo_config(&mut config, &geo);
        assert_eq!(
            config["geox-url"]["mmdb"],
            Value::String("https://cdn.example.com/geoip.metadb".into())
        );
        assert!(config["geox-url"]["geosite"]
            .as_str()
            .unwrap()
            .starts_with("https://gh-proxy.com/"));
    }

    #[test]
    fn sniffer_details_merge_over_profile() {
        let mut sniffer_cfg = crate::config::SnifferConfig::default();
        sniffer_cfg.force_dns_mapping = Some(true);
        sniffer_cfg.override_destination = Some(false);
        sniffer_cfg.http_ports = vec!["80".into(), "8080-8880".into()];
        sniffer_cfg.tls_ports = vec!["443".into()];
        let mut config: Mapping =
            serde_yaml_ng::from_str("sniffer: {enable: true, skip-domain: ['+.qq.com']}\n").unwrap();
        apply_sniffer_config(&mut config, true, &sniffer_cfg);
        assert_eq!(config["sniffer"]["enable"], Value::Bool(true));
        assert_eq!(config["sniffer"]["force-dns-mapping"], Value::Bool(true));
        assert_eq!(config["sniffer"]["override-destination"], Value::Bool(false));
        assert!(!config["sniffer"].as_mapping().unwrap().contains_key("parse-pure-ip"));
        assert_eq!(config["sniffer"]["sniff"]["HTTP"]["ports"][0], Value::Number(80.into()));
        assert_eq!(
            config["sniffer"]["sniff"]["HTTP"]["ports"][1],
            Value::String("8080-8880".into())
        );
        assert_eq!(config["sniffer"]["sniff"]["TLS"]["ports"][0], Value::Number(443.into()));
        // Profile-provided keys survive the merge.
        assert_eq!(config["sniffer"]["skip-domain"][0], Value::String("+.qq.com".into()));
    }

    #[test]
    fn runtime_defaults_store_selected_nodes() {        let mut config: Mapping = serde_yaml_ng::from_str("tun: {enable: true}\n").unwrap();
        let cfg = crate::config::Config::default();
        apply_runtime_defaults(&mut config, &cfg);
        assert_eq!(config["profile"]["store-selected"], Value::Bool(true));
        assert!(!config.contains_key("tun"));
    }

    #[test]
    fn runtime_defaults_apply_ports_and_tun() {
        let mut cfg = crate::config::Config::default();
        cfg.socks_port = Some(7891);
        cfg.authentication = vec!["admin:secret".into()];
        cfg.tcp_concurrent = Some(true);
        cfg.tun.enable = true;
        cfg.tun.stack = Some("gVisor".into());
        cfg.tun.mtu = Some(9000);
        cfg.tun.strict_route = Some(true);
        cfg.tun.auto_redirect = Some(false);
        cfg.tun.route_exclude_address = vec!["192.168.0.0/16".into()];
        cfg.find_process_mode = Some("always".into());
        let mut config: Mapping = serde_yaml_ng::from_str("tun: {enable: false}\n").unwrap();
        apply_runtime_defaults(&mut config, &cfg);
        assert_eq!(config["socks-port"], Value::Number(7891.into()));
        assert_eq!(config["tcp-concurrent"], Value::Bool(true));
        assert!(!config.contains_key("port"));
        assert_eq!(config["tun"]["enable"], Value::Bool(true));
        assert_eq!(config["tun"]["stack"], Value::String("gVisor".into()));
        assert_eq!(config["tun"]["mtu"], Value::Number(9000.into()));
        assert_eq!(config["tun"]["strict-route"], Value::Bool(true));
        assert_eq!(config["tun"]["auto-redirect"], Value::Bool(false));
        assert_eq!(
            config["tun"]["route-exclude-address"][0],
            Value::String("192.168.0.0/16".into())
        );
        assert_eq!(
            config["find-process-mode"],
            Value::String("always".into())
        );
    }
}
