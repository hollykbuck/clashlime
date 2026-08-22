use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const GITHUB_API_LATEST: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";
const CACHE_TTL_SECS: u64 = 6 * 3600; // 6h
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Cache {
    release: GithubRelease,
    checked_at: u64,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateState {
    pub current: String,
    pub latest: Option<String>,
    pub html_url: Option<String>,
    pub available: Option<bool>, // None = unknown/not checked, Some(true)=update available
    pub message: String,
    pub checking: bool,
    pub checked_at: Option<u64>,
    pub prerelease: bool,
}

impl UpdateState {
    pub fn status_text(&self) -> String {
        if self.checking {
            return "checking…".into();
        }
        if let Some(avail) = self.available {
            if avail {
                return "update available".into();
            } else {
                return "up to date".into();
            }
        }
        if self.message.is_empty() {
            "not checked".into()
        } else {
            self.message.clone()
        }
    }
}

fn cache_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("omash/update-check.json")
}

fn github_repo_api() -> String {
    std::env::var("OMASH_MIHOMO_REPO")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|r| {
            let r = r.trim().trim_matches('/').to_owned();
            // allow "owner/repo" → build api url
            if r.starts_with("http") {
                r
            } else {
                format!("https://api.github.com/repos/{r}/releases/latest")
            }
        })
        .unwrap_or_else(|| GITHUB_API_LATEST.to_owned())
}

pub fn current_version_from_binary() -> Result<String> {
    let path = crate::config::Config::mihomo_path();
    if !path.is_file() {
        bail!("mihomo binary not found at {}", path.display());
    }
    let output = std::process::Command::new(&path)
        .arg("-v")
        .output()
        .with_context(|| format!("failed to run {} -v", path.display()))?;
    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);
    parse_version_from_text(&text).with_context(|| format!("cannot parse version from: {text}"))
}

fn parse_version_from_text(text: &str) -> Result<String> {
    // mihomo prints: "Mihomo Meta v1.19.30 linux amd64..." or "v1.19.30"
    // Extract `v\d+\.\d+\.\d+...`
    let re_like = text
        .split_whitespace()
        .find(|w| {
            let t = w.trim_matches(|c: char| {
                !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != 'v' && c != 'V'
            });
            // Look for pattern starting with v
            let t = t.trim_matches(',').trim();
            (t.starts_with('v') || t.starts_with('V')) && t.chars().any(|c| c == '.')
        })
        .or_else(|| {
            // fallback regex-ish scan
            text.split(|c: char| {
                !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != 'v' && c != 'V'
            })
            .find(|w| w.starts_with('v') && w.contains('.'))
        });
    if let Some(v) = re_like {
        let v = v
            .trim()
            .trim_matches(|c: char| c == ',' || c == '"' || c == '\'')
            .to_owned();
        // normalize already
        if v.starts_with('v') || v.starts_with('V') {
            return Ok(v);
        }
    }
    // More robust scan
    for token in text.split_whitespace() {
        let token = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != 'v');
        if token.starts_with('v') && token.contains('.') {
            // strip trailing punct
            let clean = token
                .trim_end_matches(|c: char| c == ',' || c == ')' || c == ']')
                .to_owned();
            return Ok(clean);
        }
    }
    bail!("no version found in text")
}

pub fn normalize_version(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('v')
        .or_else(|| v.strip_prefix('V'))
        .unwrap_or(v);
    v.to_owned()
}

pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let a = normalize_version(a);
    let b = normalize_version(b);
    // split by '.' and '-' (prerelease)
    let parse = |s: &str| {
        s.split(|c: char| c == '.' || c == '-' || c == '+')
            .filter(|p| !p.is_empty())
            .map(|p| {
                // numeric prefix
                let num: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                num.parse::<u64>().unwrap_or(0)
            })
            .collect::<Vec<_>>()
    };
    let av = parse(&a);
    let bv = parse(&b);
    let max = av.len().max(bv.len());
    for i in 0..max {
        let ai = *av.get(i).unwrap_or(&0);
        let bi = *bv.get(i).unwrap_or(&0);
        match ai.cmp(&bi) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    // If numeric equal, check prerelease tag: "alpha" < release
    let a_has_pre = a.contains('-');
    let b_has_pre = b.contains('-');
    match (a_has_pre, b_has_pre) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_cache() -> Option<Cache> {
    let path = cache_path();
    let text = fs::read_to_string(&path).ok()?;
    let cache: Cache = serde_json::from_str(&text).ok()?;
    Some(cache)
}

fn save_cache(release: &GithubRelease) -> Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache = Cache {
        release: release.clone(),
        checked_at: now_secs(),
    };
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&cache)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

pub async fn fetch_latest_release(force: bool) -> Result<GithubRelease> {
    if !force {
        if let Some(cache) = load_cache() {
            if now_secs().saturating_sub(cache.checked_at) < CACHE_TTL_SECS {
                return Ok(cache.release);
            }
        }
    }
    let api = github_repo_api();
    let client = reqwest::Client::builder()
        .no_proxy()
        .user_agent(format!("omash/{}", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let mut req = client
        .get(&api)
        .header("Accept", "application/vnd.github.v3+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        if !token.trim().is_empty() {
            req = req.header("Authorization", format!("Bearer {}", token.trim()));
        }
    }
    let resp = req.send().await.context("failed to fetch GitHub release")?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        // Try to parse rate limit message
        if status.as_u16() == 403 && text.to_lowercase().contains("rate limit") {
            bail!(
                "GitHub API rate limited (403). Set $GITHUB_TOKEN or try later. Response: {text}"
            );
        }
        if status.as_u16() == 404 {
            bail!("GitHub release not found (404): {api}");
        }
        bail!("GitHub API {status}: {text}");
    }
    let release: GithubRelease =
        serde_json::from_str(&text).with_context(|| format!("invalid release json: {text}"))?;
    if release.tag_name.is_empty() {
        bail!("GitHub release missing tag_name");
    }
    let _ = save_cache(&release);
    Ok(release)
}

pub async fn check_update(current: &str, force: bool) -> Result<(GithubRelease, bool)> {
    let latest = fetch_latest_release(force).await?;
    let available = compare_versions(current, &latest.tag_name) == Ordering::Less;
    Ok((latest, available))
}

pub fn clear_cache() -> Result<()> {
    let path = cache_path();
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_text() {
        assert_eq!(
            parse_version_from_text("Mihomo Meta v1.19.30 linux amd64 with go1.26.6").unwrap(),
            "v1.19.30"
        );
        assert_eq!(parse_version_from_text("v1.19.31").unwrap(), "v1.19.31");
        assert_eq!(
            parse_version_from_text("version v2.0.0-alpha").unwrap(),
            "v2.0.0-alpha"
        );
    }

    #[test]
    fn compare_semver() {
        assert_eq!(compare_versions("v1.19.30", "v1.19.31"), Ordering::Less);
        assert_eq!(compare_versions("v1.19.31", "v1.19.30"), Ordering::Greater);
        assert_eq!(compare_versions("v1.19.30", "v1.19.30"), Ordering::Equal);
        assert_eq!(compare_versions("1.19.30", "v1.19.30"), Ordering::Equal);
        assert_eq!(compare_versions("v1.19.0", "v1.19"), Ordering::Equal);
        assert_eq!(compare_versions("v1.20.0", "v1.19.99"), Ordering::Greater);
        assert_eq!(
            compare_versions("v1.19.30-alpha", "v1.19.30"),
            Ordering::Less
        );
    }

    #[test]
    fn normalize_strips_v() {
        assert_eq!(normalize_version("v1.19.30"), "1.19.30");
        assert_eq!(normalize_version("V1.19.30 "), "1.19.30");
        assert_eq!(normalize_version("1.19.30"), "1.19.30");
    }
}
