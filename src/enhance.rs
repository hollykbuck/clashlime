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

pub fn apply_runtime_defaults(
    config: &mut Mapping,
    controller: &str,
    secret: &str,
    mixed_port: u16,
    allow_lan: bool,
    ipv6: bool,
) {
    let controller = controller
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    set(config, "external-controller", controller);
    set(config, "secret", secret);
    set(config, "mixed-port", mixed_port);
    set(config, "allow-lan", allow_lan);
    set(config, "ipv6", ipv6);
    config.remove("tun");
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
    // Preserve other dns keys from profile (e.g., fallback-filter) via deep merge
    if let Some(Value::Mapping(existing)) = config.get(&Value::String("dns".into())).cloned() {
        let mut merged = existing;
        deep_merge(&mut merged, dns_map);
        config.insert(Value::String("dns".into()), Value::Mapping(merged));
    } else {
        config.insert(Value::String("dns".into()), Value::Mapping(dns_map));
    }
}

/// Inject a minimal sniffer override; profile-provided sniffer keys are
/// preserved via deep merge, mirroring the DNS override behavior.
pub fn apply_sniffer_config(config: &mut Mapping, enabled: bool) {
    if !enabled {
        return;
    }
    let mut sniffer = Mapping::new();
    sniffer.insert(Value::String("enable".into()), Value::Bool(true));
    if let Some(Value::Mapping(existing)) = config
        .get(&Value::String("sniffer".into()))
        .cloned()
    {
        let mut merged = existing;
        deep_merge(&mut merged, sniffer);
        config.insert(Value::String("sniffer".into()), Value::Mapping(merged));
    } else {
        config.insert(Value::String("sniffer".into()), Value::Mapping(sniffer));
    }
}

/// Inject geo database settings so the running core keeps them fresh
/// itself (mirrors clash-party's controled `geox-url` / `geo-auto-update`).
/// URLs honor the configured mirror; validation still uses the files in the
/// data dir ensured by [`crate::geo::ensure_for_content`].
pub fn apply_geo_config(config: &mut Mapping, geo: &crate::config::GeoConfig) {
    let mirror = crate::geo::effective_mirror(geo.mirror.as_deref());
    let url = |asset: &str| match mirror.as_deref() {
        Some(m) => format!("{}/{asset}", m.trim_end_matches('/')),
        None => asset.to_owned(),
    };
    let mut geox = Mapping::new();
    geox.insert(
        Value::String("geoip".into()),
        Value::String(url("https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip-lite.dat").into()),
    );
    geox.insert(
        Value::String("geosite".into()),
        Value::String(url("https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat").into()),
    );
    geox.insert(
        Value::String("mmdb".into()),
        Value::String(url("https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb").into()),
    );
    geox.insert(
        Value::String("asn".into()),
        Value::String(url("https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/GeoLite2-ASN.mmdb").into()),
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
    fn runtime_defaults_store_selected_nodes() {
        let mut config: Mapping = serde_yaml_ng::from_str("tun: {enable: true}\n").unwrap();
        apply_runtime_defaults(
            &mut config,
            "http://127.0.0.1:9090",
            "secret",
            7897,
            false,
            true,
        );
        assert_eq!(config["profile"]["store-selected"], Value::Bool(true));
        assert!(!config.contains_key("tun"));
    }
}
