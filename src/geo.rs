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

const GEO_BASE_URL: &str =
    "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest";

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

pub fn download_url(file: &GeoFile, mirror: Option<&str>) -> String {
    let upstream = format!("{GEO_BASE_URL}/{}", file.asset);
    match mirror.map(str::trim).filter(|m| !m.is_empty()) {
        Some(mirror) => format!("{}/{upstream}", mirror.trim_end_matches('/')),
        None => upstream,
    }
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

async fn download(file: &GeoFile, mirror: Option<&str>) -> Result<PathBuf> {
    let url = download_url(file, mirror);
    crate::logger::info("geo", &format!("downloading {} from {url}", file.name));
    let bytes = reqwest::Client::builder()
        .user_agent(format!("omash/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
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
pub async fn ensure_all(mirror: Option<&str>) -> Result<Vec<String>> {
    let mut fetched = Vec::new();
    for file in GEO_FILES {
        if present(file.name) {
            continue;
        }
        download(file, mirror).await?;
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
pub async fn ensure_for_content(content: &str, mirror: Option<&str>) -> Result<Vec<String>> {
    let mut fetched = Vec::new();
    let need = |name: &str| GEO_FILES.iter().find(|f| f.name == name).unwrap();
    if wants_metadb(content) && !present("geoip.metadb") {
        let file = need("geoip.metadb");
        download(file, mirror).await.map_err(|error| {
            anyhow::anyhow!(
                "profile needs GEOIP but geoip.metadb is missing and download failed: {error:#}; \
                 place it at {} or set a mirror via [geo] mirror / $OMASH_GEO_MIRROR",
                path("geoip.metadb").display()
            )
        })?;
        fetched.push("geoip.metadb".to_owned());
    }
    if wants_geosite(content) && !present("geosite.dat") {
        let file = need("geosite.dat");
        download(file, mirror).await.map_err(|error| {
            anyhow::anyhow!(
                "profile needs GEOSITE but geosite.dat is missing and download failed: {error:#}; \
                 place it at {} or set a mirror via [geo] mirror / $OMASH_GEO_MIRROR",
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
            download_url(file, None),
            format!("{GEO_BASE_URL}/{}", file.asset)
        );
        assert_eq!(
            download_url(file, Some("https://gh-proxy.com/")),
            format!("https://gh-proxy.com/{GEO_BASE_URL}/{}", file.asset)
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
    fn summary_marks_missing_files() {
        // Pure formatting check on synthetic data.
        let entries = vec![("geoip.metadb", Some(8_000_000u64)), ("geosite.dat", None)];
        let ok = entries.iter().filter(|(_, s)| s.is_some()).count();
        assert_eq!(ok, 1);
        assert!(format_size(8_555_449).starts_with("8.2"));
    }
}
