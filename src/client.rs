//! 访问 mihomo 的 External Controller REST API。

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_TEST_URL: &str = "https://www.gstatic.com/generate_204";

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    pub time: String,
    #[serde(default)]
    pub delay: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Proxy {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub now: Option<String>,
    #[serde(default)]
    pub all: Option<Vec<String>>,
    #[serde(default)]
    pub history: Vec<History>,
    #[serde(default)]
    pub udp: bool,
}

impl Proxy {
    /// 组类型（拥有 all 列表）
    pub fn is_group(&self) -> bool {
        self.all.is_some()
    }

    pub fn last_delay(&self) -> Option<u64> {
        self.history.last().map(|h| h.delay)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Configs {
    #[serde(default)]
    pub mode: String,
    #[serde(rename = "mixed-port", default)]
    pub mixed_port: u16,
    #[serde(rename = "log-level", default)]
    pub log_level: String,
    #[serde(rename = "allow-lan", default)]
    pub allow_lan: bool,
    #[serde(default)]
    pub ipv6: bool,
    #[serde(default)]
    pub port: u16,
    #[serde(rename = "socks-port", default)]
    pub socks_port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub payload: String,
    #[serde(default)]
    pub proxy: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Provider {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "vehicleType", default)]
    pub vehicle_type: String,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: String,
    #[serde(default)]
    pub proxies: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Traffic {
    #[serde(default)]
    pub up: u64,
    #[serde(default)]
    pub down: u64,
    #[serde(rename = "upTotal", default)]
    pub up_total: u64,
    #[serde(rename = "downTotal", default)]
    pub down_total: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Connection {
    pub id: String,
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub chains: Vec<String>,
    #[serde(default)]
    pub rule: String,
    #[serde(default)]
    pub rule_payload: String,
    #[serde(default)]
    pub metadata: ConnectionMeta,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConnectionMeta {
    #[serde(default)]
    pub host: String,
    #[serde(rename = "destinationIP", default)]
    pub destination_ip: String,
    #[serde(rename = "destinationPort", default)]
    pub destination_port: String,
    #[serde(rename = "sourceIP", default)]
    pub source_ip: String,
    #[serde(rename = "network", default)]
    pub network: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(rename = "processPath", default)]
    pub process_path: String,
}

impl Client {
    pub fn new(url: &str, secret: &str) -> Result<Self> {
        let base = url.trim().trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            bail!("控制器地址必须以 http:// 或 https:// 开头");
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Client {
            http,
            base,
            secret: secret.to_string(),
        })
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut builder = self.http.request(method, format!("{}{}", self.base, path));
        if !self.secret.is_empty() {
            builder = builder.bearer_auth(&self.secret);
        }
        builder
    }

    async fn value(&self, builder: reqwest::RequestBuilder) -> Result<serde_json::Value> {
        let response = builder.send().await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let hint = match status.as_u16() {
                401 => "（密钥错误，请在“设置”里填写正确的 secret）",
                404 => "（该内核不支持此接口）",
                405 => "（该内核不允许此操作）",
                _ => "",
            };
            let body = String::from_utf8_lossy(&bytes);
            let body = body.trim();
            let detail = if body.is_empty() { "" } else { body };
            bail!("控制器返回 HTTP {} {hint} {detail}", status.as_u16());
        }
        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let value = self.value(self.req(reqwest::Method::GET, path)).await?;
        Ok(serde_json::from_value(value)?)
    }

    fn encode(name: &str) -> String {
        // 简易百分号编码，覆盖组/节点名里可能出现的字符。
        let mut out = String::new();
        for byte in name.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char)
                }
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    }

    pub async fn version(&self) -> Result<String> {
        let value = self.value(self.req(reqwest::Method::GET, "/version")).await?;
        Ok(value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    pub async fn configs(&self) -> Result<Configs> {
        self.get_json("/configs").await
    }

    pub async fn set_mode(&self, mode: &str) -> Result<()> {
        self.value(
            self.req(reqwest::Method::PATCH, "/configs")
                .json(&serde_json::json!({ "mode": mode })),
        )
        .await?;
        Ok(())
    }

    pub async fn reload(&self, path: &str) -> Result<()> {
        self.value(
            self.req(reqwest::Method::PUT, "/configs?force=true")
                .json(&serde_json::json!({ "path": path })),
        )
        .await?;
        Ok(())
    }

    pub async fn proxies(&self) -> Result<HashMap<String, Proxy>> {
        let value = self.value(self.req(reqwest::Method::GET, "/proxies")).await?;
        let map = value
            .get("proxies")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        Ok(serde_json::from_value(map)?)
    }

    pub async fn select(&self, group: &str, node: &str) -> Result<()> {
        self.value(
            self.req(
                reqwest::Method::PUT,
                &format!("/proxies/{}", Self::encode(group)),
            )
            .json(&serde_json::json!({ "name": node })),
        )
        .await?;
        Ok(())
    }

    pub async fn delay(&self, node: &str, url: &str, timeout_ms: u64) -> Result<u64> {
        let path = format!(
            "/proxies/{}/delay?url={}&timeout={}",
            Self::encode(node),
            Self::encode(url),
            timeout_ms
        );
        let value = self.value(self.req(reqwest::Method::GET, &path)).await?;
        let delay = value
            .get("delay")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Ok(delay)
    }

    pub async fn group_delay(&self, group: &str, url: &str, timeout_ms: u64) -> Result<HashMap<String, u64>> {
        let path = format!(
            "/group/{}/delay?url={}&timeout={}",
            Self::encode(group),
            Self::encode(url),
            timeout_ms
        );
        let value = self.value(self.req(reqwest::Method::GET, &path)).await?;
        let mut out = HashMap::new();
        if let Some(object) = value.as_object() {
            for (key, entry) in object {
                if let Some(delay) = entry.get("delay").and_then(|v| v.as_u64()) {
                    out.insert(key.clone(), delay);
                }
            }
        }
        Ok(out)
    }

    pub async fn providers(&self) -> Result<HashMap<String, Provider>> {
        let value = self
            .value(self.req(reqwest::Method::GET, "/providers/proxies"))
            .await?;
        let map = value
            .get("providers")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        Ok(serde_json::from_value(map)?)
    }

    pub async fn update_provider(&self, name: &str) -> Result<()> {
        self.value(
            self.req(
                reqwest::Method::PUT,
                &format!("/providers/proxies/{}", Self::encode(name)),
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn provider_healthcheck(&self, name: &str) -> Result<()> {
        self.value(
            self.req(
                reqwest::Method::GET,
                &format!("/providers/proxies/{}/healthcheck", Self::encode(name)),
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn rules(&self) -> Result<Vec<Rule>> {
        let value = self.value(self.req(reqwest::Method::GET, "/rules")).await?;
        Ok(serde_json::from_value(
            value
                .get("rules")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )?)
    }

    pub async fn connections(&self) -> Result<Vec<Connection>> {
        let value = self
            .value(self.req(reqwest::Method::GET, "/connections"))
            .await?;
        Ok(serde_json::from_value(
            value
                .get("connections")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![])),
        )?)
    }

    pub async fn close_connection(&self, id: &str) -> Result<()> {
        self.value(
            self.req(reqwest::Method::DELETE, &format!("/connections/{}", Self::encode(id))),
        )
        .await?;
        Ok(())
    }

    /// 关闭全部活动连接（`DELETE /connections`）。
    pub async fn close_all_connections(&self) -> Result<()> {
        self.value(self.req(reqwest::Method::DELETE, "/connections"))
            .await?;
        Ok(())
    }

    /// 打开一个流式接口（`/traffic`、`/logs`），返回未读完的响应。
    pub async fn stream(&self, path: &str) -> Result<reqwest::Response> {
        let response = self.req(reqwest::Method::GET, path).send().await?;
        if !response.status().is_success() {
            bail!("流式接口 {path} 返回 HTTP {}", response.status().as_u16());
        }
        Ok(response)
    }
}
