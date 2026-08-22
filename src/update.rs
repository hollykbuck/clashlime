use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::AsyncWriteExt;

const GITHUB_API_LATEST: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";
const CACHE_TTL_SECS: u64 = 6 * 3600; // 6h
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

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
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    /// GitHub's API field is `browser_download_url`; the alias keeps cached
    /// copies round-tripping through our own `download_url` key.
    #[serde(alias = "browser_download_url")]
    pub download_url: String,
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
    version_from_binary_at(&path)
}

/// Run `mihomo -v` on an explicit binary and extract its version string.
pub fn version_from_binary_at(path: &Path) -> Result<String> {
    if !path.is_file() {
        bail!("mihomo binary not found at {}", path.display());
    }
    let output = std::process::Command::new(path)
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

/// Pick the release asset matching this machine's OS and architecture.
pub fn pick_asset(release: &GithubRelease) -> Result<GithubAsset> {
    // Rust calls it "macos"; mihomo assets use the Darwin name.
    let os = if std::env::consts::OS == "macos" {
        "darwin"
    } else {
        std::env::consts::OS
    };
    let arch = normalize_arch(std::env::consts::ARCH);
    let want_zip = os == "windows";
    let mut matches: Vec<&GithubAsset> = release
        .assets
        .iter()
        .filter(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.contains(&format!("{os}-{arch}"))
                && name.ends_with(if want_zip { ".zip" } else { ".gz" })
                && !name.contains("compatible")
        })
        .collect();
    // Prefer plain builds (e.g. amd64) over variant suffixes; stable sort keeps
    // version ordering intact for equally specific names.
    matches.sort_by_key(|asset| asset.name.len());
    matches.into_iter().next().cloned().with_context(|| {
        format!(
            "no mihomo asset for {os}/{arch} in release {} (found: {})",
            release.tag_name,
            release
                .assets
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

fn normalize_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Progress reported while a core download streams to disk.
#[derive(Clone, Copy, Debug)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Download `asset` from GitHub and install the decompressed binary at
/// `destination`, making it executable. Streams chunks straight to a
/// temporary file (no full in-memory buffering) and reports byte progress
/// through `progress` when provided. Returns the destination path.
pub async fn download_core(
    asset: &GithubAsset,
    destination: &Path,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<DownloadProgress>>,
) -> Result<PathBuf> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .user_agent(format!("omash/{}", env!("CARGO_PKG_VERSION")))
        .timeout(DOWNLOAD_TIMEOUT)
        .build()?;
    let response = client
        .get(&asset.download_url)
        .send()
        .await
        .with_context(|| format!("failed to download {}", asset.download_url))?;
    let status = response.status();
    if !status.is_success() {
        bail!("download failed with HTTP {status}: {}", asset.download_url);
    }
    let total = response.content_length().filter(|size| *size > 0);

    let parent = destination.parent().ok_or_else(|| {
        anyhow::anyhow!("invalid destination {}", destination.display())
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let temporary = destination.with_extension("download");
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .with_context(|| format!("failed to create {}", temporary.display()))?;

    let mut downloaded: u64 = 0;
    let mut last_reported: u64 = 0;
    const REPORT_STEP: u64 = 256 * 1024;
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed to read download stream")?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        downloaded += chunk.len() as u64;
        if downloaded.saturating_sub(last_reported) >= REPORT_STEP
            && let Some(sender) = progress
        {
            last_reported = downloaded;
            let _ = sender.send(DownloadProgress { downloaded, total });
        }
    }
    file.flush()
        .await
        .context("failed to flush download buffer")?;
    drop(file);
    if downloaded == 0 {
        let _ = fs::remove_file(&temporary);
        bail!("downloaded file is empty");
    }
    if let Some(sender) = progress {
        let _ = sender.send(DownloadProgress { downloaded, total: Some(downloaded) });
    }

    // Decompress in place: read the temp payload back, decode, rewrite.
    let binary: Vec<u8> = if asset.name.ends_with(".gz") {
        decode_gzip_file(&temporary)?
    } else if asset.name.ends_with(".zip") {
        decode_zip_first_entry(&temporary)?
    } else {
        fs::read(&temporary).context("failed to read download payload")?
    };
    verify_elf_like(&binary)?;
    fs::write(&temporary, &binary)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    make_executable(&temporary)?;
    // Atomic-ish swap so a failed download never leaves a broken core behind
    fs::rename(&temporary, destination).with_context(|| {
        format!(
            "failed to move core into place at {}",
            destination.display()
        )
    })?;
    crate::logger::info(
        "update",
        &format!(
            "installed mihomo {} ({}) to {}",
            asset.name,
            format_size(binary.len()),
            destination.display()
        ),
    );
    Ok(destination.to_path_buf())
}

fn decode_gzip_file(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).context("failed to read download payload")?;
    let mut decoder = GzDecoder::new(file);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .context("invalid gzip payload")?;
    Ok(out)
}

fn decode_zip_first_entry(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).context("failed to read download payload")?;
    let mut archive = zip::ZipArchive::new(file).context("invalid zip payload")?;
    let mut entry = archive
        .by_index(0)
        .context("zip archive contains no entries")?;
    if entry.is_dir() {
        bail!("first zip entry is a directory");
    }
    let mut out = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut out)
        .context("failed to read zip entry")?;
    Ok(out)
}

/// Cheap sanity check so we never install an HTML error page as the core.
fn verify_elf_like(binary: &[u8]) -> Result<()> {
    if binary.len() < 4 {
        bail!("downloaded binary is too small");
    }
    if binary.starts_with(b"\x7fELF") || binary.starts_with(b"MZ") || binary.starts_with(b"#!") {
        return Ok(());
    }
    // Mach-O thin (32/64-bit) and fat binaries, big-endian magic
    let magic = u32::from_be_bytes([binary[0], binary[1], binary[2], binary[3]]);
    if matches!(magic, 0xFEED_FACE | 0xFEED_FACF | 0xCAFE_BABE) {
        return Ok(());
    }
    bail!("downloaded file does not look like a mihomo binary");
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn format_size(bytes: usize) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
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

    #[test]
    fn format_size_uses_human_units() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KiB");
        assert_eq!(format_size(15 * 1024 * 1024), "15.0 MiB");
    }

    fn release_with(names: &[&str]) -> GithubRelease {
        GithubRelease {
            tag_name: "v1.19.30".into(),
            html_url: "https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.30".into(),
            name: None,
            published_at: None,
            prerelease: false,
            draft: false,
            assets: names
                .iter()
                .map(|name| GithubAsset {
                    name: (*name).into(),
                    size: 0,
                    download_url: format!("https://example.com/{name}"),
                })
                .collect(),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn picks_plain_linux_asset_for_host_arch() {
        let arch = normalize_arch(std::env::consts::ARCH);
        let release = release_with(&[
            "mihomo-linux-arm-v1.19.30.gz",
            &format!("mihomo-linux-{arch}-compatible-v1.19.30.gz"),
            &format!("mihomo-linux-{arch}-v1.19.30.gz"),
            "mihomo-windows-amd64-v1.19.30.zip",
        ]);
        let asset = pick_asset(&release).expect("asset");
        assert_eq!(
            asset.name,
            format!("mihomo-linux-{arch}-v1.19.30.gz"),
            "plain build wins over -compatible"
        );
    }

    #[test]
    fn rejects_releases_without_matching_asset() {
        let release = release_with(&["mihomo-freebsd-386-v1.19.30.gz"]);
        assert!(pick_asset(&release).is_err());
    }

    #[test]
    fn verifies_binary_magic() {
        assert!(verify_elf_like(b"\x7fELFrest").is_ok());
        assert!(verify_elf_like(b"MZwin").is_ok());
        assert!(verify_elf_like(b"<html>").is_err());
        assert!(verify_elf_like(b"no").is_err());
    }

    /// End-to-end against a local HTTP server: gzip payload → decoded,
    /// chmod'ed, atomically installed binary. No network needed.
    #[tokio::test]
    async fn downloads_and_installs_from_local_server() {
        use std::io::Write as _;
        // Fake ELF payload, gzipped like GitHub ships mihomo
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"\x7fELFfake-mihomo-payload").unwrap();
        let gz = encoder.finish().unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let gz_len = gz.len();
        let server = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let mut stream = stream;
                let mut request = [0u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut request);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\n\
                     Content-Length: {gz_len}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&gz);
            }
        });

        let asset = GithubAsset {
            name: "mihomo-linux-amd64-v9.9.9.gz".into(),
            size: gz_len as u64,
            download_url: format!("http://127.0.0.1:{port}/mihomo-linux-amd64-v9.9.9.gz"),
        };
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("bin/mihomo");
        download_core(&asset, &destination, None).await.expect("install");
        assert!(destination.is_file());
        let contents = fs::read(&destination).unwrap();
        assert_eq!(contents, b"\x7fELFfake-mihomo-payload");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&destination).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "binary must be executable");
        }
        assert!(!destination.with_extension("download").exists());
        server.join().unwrap();
    }

    /// End-to-end: fetch the real latest release, install into a temp dir,
    /// then execute `mihomo -v`. Run manually: cargo test -- --ignored
    #[tokio::test]
    #[ignore = "requires network access"]
    async fn downloads_real_core_into_temp_dir() {
        let attempt = tokio::time::timeout(Duration::from_secs(120), async {
            let release = fetch_latest_release(true).await?;
            let asset = pick_asset(&release)?;
            let dir = tempfile::tempdir()?;
            let destination = dir.path().join("bin/mihomo");
            download_core(&asset, &destination, None).await?;
            anyhow::Ok((destination, dir, asset))
        })
        .await
        .expect("test must not hang; network is unreachable?")
        .expect("install");
        assert!(attempt.0.is_file());
        let version = version_from_binary_at(&attempt.0).expect("version probe");
        println!("installed {version} from {}", attempt.2.name);
    }
}
