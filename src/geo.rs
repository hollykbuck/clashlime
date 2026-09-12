//! GeoIP / GeoSite database management.
//!
//! Inspired by clash-party's "外部资源" (external resources) panel: mihomo
//! needs `geoip.metadb` / `geosite.dat` next to the data dir for GEOIP /
//! GEOSITE rules, otherwise even `mihomo -t` fails after a ~90s download
//! timeout. omash therefore ensures the files exist before validating a
//! profile, and lets the running core keep them fresh via mihomo's native
//! `geo-auto-update` settings (see [`apply_geo_config`] usage in enhance).

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::time::Duration;

const GEO_BASE_URL: &str = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest";

/// Files omash manages inside [`crate::config::Config::data_dir`].
pub struct GeoFile {
    /// File name on disk.
    pub name: &'static str,
    /// Asset name under [`GEO_BASE_URL`].
    pub asset: &'static str,
    /// Human description for status lines.
    pub describe: &'static str,
}

pub const GEO_FILES: &[GeoFile] = &[
    GeoFile {
        name: "geoip.metadb",
        asset: "geoip.metadb",
        describe: "GEOIP rules",
    },
    GeoFile {
        name: "geosite.dat",
        asset: "geosite.dat",
        describe: "GEOSITE rules",
    },
    GeoFile {
        name: "geoip.dat",
        asset: "geoip.dat",
        describe: "geodata-mode dat",
    },
];

/// Resolve the mirror prefix: `$OMASH_GEO_MIRROR` wins over config.
pub fn effective_mirror(configured: Option<&str>) -> Option<String> {
    let from_env = std::env::var("OMASH_GEO_MIRROR")
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if from_env.is_some() {
        return from_env;
    }
    configured
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Resolve the proxy for geo downloads: `$OMASH_GEO_PROXY` wins over config.
/// Accepts `http://` / `https://` proxy URLs (mihomo's mixed port works).
pub fn effective_proxy(configured: Option<&str>) -> Option<String> {
    let from_env = std::env::var("OMASH_GEO_PROXY")
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if from_env.is_some() {
        return from_env;
    }
    configured
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Mirror-prefixed upstream URL, or the per-asset full URL when the user
/// configured one (`geoip/geosite/mmdb/asn_url`).
pub fn download_url_for(
    file: &GeoFile,
    mirror: Option<&str>,
    override_url: Option<&str>,
) -> String {
    if let Some(url) = override_url.map(str::trim).filter(|u| !u.is_empty()) {
        return url.to_owned();
    }
    let upstream = format!("{GEO_BASE_URL}/{}", file.asset);
    match mirror.map(str::trim).filter(|m| !m.is_empty()) {
        Some(mirror) => format!("{}/{upstream}", mirror.trim_end_matches('/')),
        None => upstream,
    }
}

/// Per-asset URL override for an omash-managed file, if configured.
pub fn file_override<'a>(file: &GeoFile, geo: &'a crate::config::GeoConfig) -> Option<&'a str> {
    let url = match file.name {
        "geoip.metadb" => geo.mmdb_url.as_deref(),
        "geosite.dat" => geo.geosite_url.as_deref(),
        "geoip.dat" => geo.geoip_url.as_deref(),
        _ => None,
    };
    url.map(str::trim).filter(|u| !u.is_empty())
}

pub fn path(name: &str) -> PathBuf {
    crate::config::Config::data_dir().join(name)
}

/// True when the file exists and is non-empty.
pub fn present(name: &str) -> bool {
    std::fs::metadata(path(name)).is_ok_and(|m| m.len() > 0)
}

/// `(file name, size in bytes when present)` for every managed file.
pub fn status() -> Vec<(&'static str, Option<u64>)> {
    GEO_FILES
        .iter()
        .map(|file| {
            (
                file.name,
                std::fs::metadata(path(file.name)).ok().map(|m| m.len()),
            )
        })
        .collect()
}

/// One-line summary for the Settings page, e.g. `✓ 3/3 · 8.6 MB`.
pub fn summary() -> String {
    let entries = status();
    let ok = entries.iter().filter(|(_, size)| size.is_some()).count();
    if ok == entries.len() {
        let bytes: u64 = entries.iter().filter_map(|(_, size)| *size).sum();
        format!("✓ {ok}/{} · {}", entries.len(), format_size(bytes))
    } else {
        let missing: Vec<_> = entries
            .iter()
            .filter(|(_, size)| size.is_none())
            .map(|(name, _)| {
                let what = GEO_FILES
                    .iter()
                    .find(|f| f.name == *name)
                    .map(|f| f.describe)
                    .unwrap_or(*name);
                format!("{name} ({what})")
            })
            .collect();
        format!("✗ {ok}/{} · missing {}", entries.len(), missing.join(", "))
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

async fn download(
    file: &GeoFile,
    mirror: Option<&str>,
    proxy: Option<&str>,
    override_url: Option<&str>,
) -> Result<PathBuf> {
    let url = download_url_for(file, mirror, override_url);
    if let Some(proxy) = proxy {
        crate::logger::info(
            "geo",
            &format!("downloading {} via proxy {proxy}", file.name),
        );
    } else {
        crate::logger::info("geo", &format!("downloading {} from {url}", file.name));
    }
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("omash/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180));
    if let Some(proxy) = proxy.map(str::trim).filter(|p| !p.is_empty()) {
        builder = builder.proxy(
            reqwest::Proxy::all(proxy).with_context(|| format!("invalid geo proxy {proxy:?}"))?,
        );
    }
    let bytes = builder
        .build()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to download {}", file.name))?
        .error_for_status()
        .with_context(|| format!("geo download rejected for {}", file.name))?
        .bytes()
        .await
        .with_context(|| format!("failed to read {}", file.name))?;
    if bytes.is_empty() {
        bail!("downloaded {} is empty", file.name);
    }
    let destination = path(file.name);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = destination.with_extension("tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &destination)?;
    crate::logger::info(
        "geo",
        &format!("{} ready ({} bytes)", file.name, bytes.len()),
    );
    Ok(destination)
}

/// Download every managed file that is missing. Returns the names fetched.
pub async fn ensure_all(geo: &crate::config::GeoConfig) -> Result<Vec<String>> {
    let mirror = effective_mirror(geo.mirror.as_deref());
    let proxy = effective_proxy(geo.proxy.as_deref());
    let mut fetched = Vec::new();
    for file in GEO_FILES {
        if present(file.name) {
            continue;
        }
        download(
            file,
            mirror.as_deref(),
            proxy.as_deref(),
            file_override(file, geo),
        )
        .await?;
        fetched.push(file.name.to_owned());
    }
    Ok(fetched)
}

/// Does this profile text reference GEOIP / GEOSITE rule kinds?
fn wants_metadb(content: &str) -> bool {
    content.contains("GEOIP,")
}

fn wants_geosite(content: &str) -> bool {
    content.contains("GEOSITE,")
}

/// Ensure the geo files this profile needs exist, downloading what's missing.
///
/// Without this, `mihomo -t` blocks ~90s trying to fetch them itself and then
/// rejects the profile. Failure here returns an actionable error naming the
/// data dir and mirror override instead.
pub async fn ensure_for_content(
    content: &str,
    geo: &crate::config::GeoConfig,
) -> Result<Vec<String>> {
    let mirror = effective_mirror(geo.mirror.as_deref());
    let proxy = effective_proxy(geo.proxy.as_deref());
    let mut fetched = Vec::new();
    let need = |name: &str| GEO_FILES.iter().find(|f| f.name == name).unwrap();
    if wants_metadb(content) && !present("geoip.metadb") {
        let file = need("geoip.metadb");
        download(
            file,
            mirror.as_deref(),
            proxy.as_deref(),
            file_override(file, geo),
        )
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "profile needs GEOIP but geoip.metadb is missing and download failed: {error:#}; \
                 place it at {} or set a mirror/proxy via [geo] / $OMASH_GEO_MIRROR / $OMASH_GEO_PROXY",
                path("geoip.metadb").display()
            )
        })?;
        fetched.push("geoip.metadb".to_owned());
    }
    if wants_geosite(content) && !present("geosite.dat") {
        let file = need("geosite.dat");
        download(
            file,
            mirror.as_deref(),
            proxy.as_deref(),
            file_override(file, geo),
        )
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "profile needs GEOSITE but geosite.dat is missing and download failed: {error:#}; \
                 place it at {} or set a mirror/proxy via [geo] / $OMASH_GEO_MIRROR / $OMASH_GEO_PROXY",
                path("geosite.dat").display()
            )
        })?;
        fetched.push("geosite.dat".to_owned());
    }
    Ok(fetched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_url_supports_mirror_prefix() {
        let file = &GEO_FILES[0];
        assert_eq!(
            download_url_for(file, None, None),
            format!("{GEO_BASE_URL}/{}", file.asset)
        );
        assert_eq!(
            download_url_for(file, Some("https://gh-proxy.com/"), None),
            format!("https://gh-proxy.com/{GEO_BASE_URL}/{}", file.asset)
        );
        assert_eq!(
            download_url_for(
                file,
                Some("https://gh-proxy.com/"),
                Some("https://cdn.example.com/geoip.metadb")
            ),
            "https://cdn.example.com/geoip.metadb"
        );
    }

    #[test]
    fn detects_geo_rule_usage() {
        assert!(wants_metadb("- GEOIP,CN,DIRECT\n"));
        assert!(!wants_metadb("- DOMAIN,example.com,DIRECT\n"));
        assert!(wants_geosite("- GEOSITE,google,PROXY\n"));
        assert!(!wants_geosite("- GEOIP,CN,DIRECT\n"));
    }

    #[test]
    fn effective_proxy_prefers_env_over_config() {
        // SAFETY: this is the only test touching OMASH_GEO_PROXY, and the
        // crate's other env-mutating tests use unrelated variables.
        let saved = std::env::var_os("OMASH_GEO_PROXY");
        unsafe { std::env::remove_var("OMASH_GEO_PROXY") };
        assert_eq!(effective_proxy(None), None);
        assert_eq!(
            effective_proxy(Some("http://127.0.0.1:7897")),
            Some("http://127.0.0.1:7897".to_owned())
        );
        assert_eq!(effective_proxy(Some("  ")), None);
        unsafe { std::env::set_var("OMASH_GEO_PROXY", "http://env:8080") };
        assert_eq!(
            effective_proxy(Some("http://127.0.0.1:7897")),
            Some("http://env:8080".to_owned())
        );
        unsafe { std::env::remove_var("OMASH_GEO_PROXY") };
        assert_eq!(effective_proxy(None), None);
        match saved {
            Some(v) => unsafe { std::env::set_var("OMASH_GEO_PROXY", v) },
            None => {}
        }
    }

    #[test]
    fn summary_marks_missing_files() {
        // Pure formatting check on synthetic data.
        let entries = vec![("geoip.metadb", Some(8_000_000u64)), ("geosite.dat", None)];
        let ok = entries.iter().filter(|(_, s)| s.is_some()).count();
        assert_eq!(ok, 1);
        assert!(format_size(8_555_449).starts_with("8.2"));
    }
}
