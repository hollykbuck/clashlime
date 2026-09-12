use anyhow::{Context, Result, bail};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub struct MihomoClient {
    client: Client,
    base: Url,
    secret: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct VersionInfo {
    #[serde(default)]
    pub version: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MemoryInfo {
    #[serde(default, alias = "in_use")]
    pub inuse: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    pub ipv6: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProxyResponse {
    #[serde(default)]
    pub proxies: HashMap<String, Proxy>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Proxy {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub now: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub all: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub history: Vec<DelayHistory>,
    #[serde(default)]
    pub alive: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DelayHistory {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub delay: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConnectionResponse {
    #[serde(rename = "downloadTotal", alias = "download_total", default)]
    pub download_total: u64,
    #[serde(rename = "uploadTotal", alias = "upload_total", default)]
    pub upload_total: u64,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub connections: Vec<Connection>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Connection {
    #[serde(default)]
    pub id: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub metadata: Metadata,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub chains: Vec<String>,
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Metadata {
    #[serde(
        rename = "host",
        alias = "hostname",
        default,
        deserialize_with = "deserialize_null_default"
    )]
    pub host: String,
    #[serde(
        rename = "destinationIP",
        default,
        deserialize_with = "deserialize_null_default"
    )]
    pub destination_ip: String,
    #[serde(
        rename = "destinationPort",
        default,
        deserialize_with = "deserialize_null_default"
    )]
    pub destination_port: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub network: String,
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_null_default"
    )]
    pub kind: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RuleResponse {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Rule {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub payload: String,
    #[serde(default)]
    pub proxy: String,
}

#[derive(Debug, Deserialize)]
struct DelayResponse {
    delay: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub version: VersionInfo,
    pub config: RuntimeConfig,
    pub proxies: ProxyResponse,
    pub connections: ConnectionResponse,
    pub rules: RuleResponse,
    /// Best-effort; `None` when the core did not answer /memory.
    pub memory: Option<MemoryInfo>,
}

/// Slowly-changing snapshot data, refreshed on a long cadence.
pub struct SlowSnapshot {
    pub rules: RuleResponse,
    pub memory: Option<MemoryInfo>,
}

impl MihomoClient {
    pub fn new(controller: &str, secret: String) -> Result<Self> {
        let base = Url::parse(&format!("{}/", controller.trim_end_matches('/')))
            .context("controller must be a valid HTTP URL")?;
        Ok(Self {
            client: Client::builder()
                // The controller belongs to the local Mihomo process managed by omash. In
                // particular, never inherit HTTP_PROXY/ALL_PROXY here: doing
                // so sends 127.0.0.1 requests to an upstream proxy and turns
                // an otherwise healthy Mihomo into an apparent 502.
                .no_proxy()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(5))
                .build()?,
            base,
            secret,
        })
    }

    fn url(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("controller URL cannot be a base"))?
            .pop_if_empty()
            .extend(segments);
        Ok(url)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
    ) -> Result<T> {
        let mut request = self.client.request(method, self.url(segments)?);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("cannot connect to Mihomo")?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            bail!(
                "Mihomo returned {status}: {}",
                String::from_utf8_lossy(&bytes)
            );
        }
        if bytes.is_empty() {
            return serde_json::from_value(Value::Null).context("empty response");
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            anyhow::anyhow!(
                "invalid response from Mihomo at /{}: {error}",
                segments.join("/")
            )
        })
    }

    async fn empty(&self, method: Method, segments: &[&str], body: Option<Value>) -> Result<()> {
        let mut request = self.client.request(method, self.url(segments)?);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("cannot connect to Mihomo")?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "Mihomo returned {status}: {}",
                response.text().await.unwrap_or_default()
            );
        }
        Ok(())
    }

    pub async fn snapshot(&self) -> Result<Snapshot> {
        let (mut snapshot, slow) = tokio::try_join!(self.snapshot_fast(), self.snapshot_slow())?;
        snapshot.rules = slow.rules;
        snapshot.memory = slow.memory;
        Ok(snapshot)
    }

    /// Fast-changing endpoints polled every tick (1–2s).
    pub async fn snapshot_fast(&self) -> Result<Snapshot> {
        let (version, config, proxies, connections) = tokio::try_join!(
            self.request(Method::GET, &["version"], None),
            self.request(Method::GET, &["configs"], None),
            self.request(Method::GET, &["proxies"], None),
            self.request(Method::GET, &["connections"], None),
        )?;
        Ok(Snapshot {
            version,
            config,
            proxies,
            connections,
            ..Snapshot::default()
        })
    }

    /// Slow endpoints: multi-MB rules dump plus `/memory` (endless stream,
    /// only the first object is read). Fetched concurrently; polled rarely,
    /// never on the hot path.
    pub async fn snapshot_slow(&self) -> Result<SlowSnapshot> {
        let (rules, memory) = tokio::try_join!(
            self.request(Method::GET, &["rules"], None),
            async { Ok(self.memory().await.ok()) },
        )?;
        Ok(SlowSnapshot { rules, memory })
    }

    pub async fn memory(&self) -> Result<MemoryInfo> {
        // Newer mihomo serves /memory as an endless stream of JSON objects
        // (one per second, like /traffic) rather than a single response, so
        // `bytes()` would wait forever. The first object arrives instantly.
        let mut request = self.client.get(self.url(&["memory"])?);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let response = request.send().await.context("cannot connect to Mihomo")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Mihomo returned {status}");
        }
        let mut response = response;
        let chunk = response
            .chunk()
            .await?
            .context("empty response from Mihomo /memory")?;
        let first = chunk
            .split(|byte| *byte == b'\n')
            .map(|line| {
                line.strip_prefix(b"\r")
                    .unwrap_or(line)
                    .strip_suffix(b"\r")
                    .unwrap_or(line)
            })
            .find(|line| !line.iter().all(|byte| byte.is_ascii_whitespace()))
            .context("empty response from Mihomo /memory")?;
        serde_json::from_slice(first).context("invalid response from Mihomo at /memory")
    }

    pub async fn version(&self) -> Result<VersionInfo> {
        self.request(Method::GET, &["version"], None).await
    }

    pub async fn runtime_config(&self) -> Result<RuntimeConfig> {
        self.request(Method::GET, &["configs"], None).await
    }

    pub async fn proxies(&self) -> Result<ProxyResponse> {
        self.request(Method::GET, &["proxies"], None).await
    }

    pub async fn select_proxy(&self, group: &str, proxy: &str) -> Result<()> {
        self.empty(
            Method::PUT,
            &["proxies", group],
            Some(json!({ "name": proxy })),
        )
        .await
    }

    pub async fn test_delay(&self, proxy: &str, target: &str) -> Result<u32> {
        let mut url = self.url(&["proxies", proxy, "delay"])?;
        url.query_pairs_mut()
            .append_pair("timeout", "5000")
            .append_pair("url", target);
        let mut request = self.client.get(url).timeout(Duration::from_secs(8));
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let response: DelayResponse = request.send().await?.error_for_status()?.json().await?;
        Ok(response.delay)
    }

    pub async fn test_group_delay(
        &self,
        group: &str,
        target: &str,
    ) -> Result<HashMap<String, u32>> {
        let mut url = self.url(&["group", group, "delay"])?;
        url.query_pairs_mut()
            .append_pair("timeout", "5000")
            .append_pair("url", target);
        let mut request = self.client.get(url).timeout(Duration::from_secs(8));
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        Ok(request.send().await?.error_for_status()?.json().await?)
    }

    pub async fn set_mode(&self, mode: &str) -> Result<()> {
        self.empty(Method::PATCH, &["configs"], Some(json!({ "mode": mode })))
            .await
    }

    pub async fn patch_configs(&self, payload: Value) -> Result<()> {
        self.empty(Method::PATCH, &["configs"], Some(payload)).await
    }

    pub async fn update_dns(&self, dns: &crate::config::DnsConfig) -> Result<()> {
        // Try hot-patch; mihomo PATCH /configs supports dns field for dynamic update.
        // Only set keys are sent so profile-provided values are preserved.
        let payload = if dns.enable {
            let mut map = serde_json::Map::new();
            map.insert("enable".into(), json!(true));
            map.insert("listen".into(), json!(dns.listen));
            map.insert("ipv6".into(), json!(dns.ipv6));
            if let Some(mode) = &dns.enhanced_mode {
                map.insert("enhanced-mode".into(), json!(mode));
            }
            if let Some(range) = &dns.fake_ip_range {
                map.insert("fake-ip-range".into(), json!(range));
            }
            if let Some(mode) = &dns.fake_ip_filter_mode {
                map.insert("fake-ip-filter-mode".into(), json!(mode));
            }
            if !dns.fake_ip_filter.is_empty() {
                map.insert("fake-ip-filter".into(), json!(dns.fake_ip_filter));
            }
            if let Some(respect) = dns.respect_rules {
                map.insert("respect-rules".into(), json!(respect));
            }
            if !dns.nameserver.is_empty() {
                map.insert("nameserver".into(), json!(dns.nameserver));
            }
            if !dns.fallback.is_empty() {
                map.insert("fallback".into(), json!(dns.fallback));
            }
            if !dns.default_nameserver.is_empty() {
                map.insert("default-nameserver".into(), json!(dns.default_nameserver));
            }
            if !dns.proxy_server_nameserver.is_empty() {
                map.insert(
                    "proxy-server-nameserver".into(),
                    json!(dns.proxy_server_nameserver),
                );
            }
            if !dns.direct_nameserver.is_empty() {
                map.insert("direct-nameserver".into(), json!(dns.direct_nameserver));
            }
            if !dns.fallback_filter.is_empty() {
                let mut filter = serde_json::Map::new();
                if let Some(geoip) = dns.fallback_filter.geoip {
                    filter.insert("geoip".into(), json!(geoip));
                }
                if let Some(code) = &dns.fallback_filter.geoip_code {
                    filter.insert("geoip-code".into(), json!(code));
                }
                if !dns.fallback_filter.ipcidr.is_empty() {
                    filter.insert("ipcidr".into(), json!(dns.fallback_filter.ipcidr));
                }
                if !dns.fallback_filter.domain.is_empty() {
                    filter.insert("domain".into(), json!(dns.fallback_filter.domain));
                }
                map.insert("fallback-filter".into(), Value::Object(filter));
            }
            json!({ "dns": Value::Object(map) })
        } else {
            json!({ "dns": { "enable": false } })
        };
        self.patch_configs(payload).await
    }

    pub async fn reload_config(&self, path: &std::path::Path) -> Result<()> {
        let mut url = self.url(&["configs"])?;
        url.query_pairs_mut().append_pair("force", "true");
        let mut request = self.client.put(url);
        if !self.secret.is_empty() {
            request = request.bearer_auth(&self.secret);
        }
        let response = request
            .json(&json!({ "path": path }))
            .send()
            .await
            .context("cannot ask Mihomo to reload configuration")?;
        let status = response.status();
        if !status.is_success() {
            bail!(
                "Mihomo returned {status} while reloading configuration: {}",
                response.text().await.unwrap_or_default()
            );
        }
        Ok(())
    }

    pub async fn close_connection(&self, id: Option<&str>) -> Result<()> {
        let path = id.map_or_else(|| vec!["connections"], |id| vec!["connections", id]);
        self.empty(Method::DELETE, &path, None).await
    }
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encodes_proxy_names() {
        let client = MihomoClient::new("http://127.0.0.1:9090", String::new()).unwrap();
        let url = client.url(&["proxies", "香港 / 01"]).unwrap();
        assert_eq!(
            url.as_str(),
            "http://127.0.0.1:9090/proxies/%E9%A6%99%E6%B8%AF%20%2F%2001"
        );
    }

    #[test]
    fn flexible_connection_metadata() {
        let value = r#"{"id":"1","metadata":{"host":"example.com","destinationPort":"443"}}"#;
        let item: Connection = serde_json::from_str(value).unwrap();
        assert_eq!(item.metadata.host, "example.com");
        assert_eq!(item.metadata.destination_port, "443");
    }

    #[test]
    fn accepts_nullable_mihomo_collections() {
        let proxies: ProxyResponse = serde_json::from_str(
            r#"{"proxies":{"node":{"type":"Compatible","now":null,"all":null}}}"#,
        )
        .unwrap();
        let node = &proxies.proxies["node"];
        assert!(node.now.is_empty());
        assert!(node.all.is_empty());

        let connections: ConnectionResponse =
            serde_json::from_str(r#"{"connections":null}"#).unwrap();
        assert!(connections.connections.is_empty());
    }
}
