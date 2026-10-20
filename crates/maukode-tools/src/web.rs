use crate::tool::{Tool, ToolError};
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

/// Structured search result entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Web search tool for querying online documentation, libraries, and technical references
pub struct WebSearchTool {
    provider: String,
    api_key: Option<String>,
    endpoint: Option<String>,
    client: reqwest::Client,
}

impl Default for WebSearchTool {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            provider: "duckduckgo".to_string(),
            api_key: None,
            endpoint: None,
            client,
        }
    }
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into().to_lowercase();
        self
    }

    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key;
        self
    }

    pub fn with_endpoint(mut self, endpoint: Option<String>) -> Self {
        self.endpoint = endpoint;
        self
    }

    async fn search_duckduckgo(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResultItem>, ToolError> {
        let resp = self
            .client
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", query), ("b", "")])
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("DuckDuckGo request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ToolError::Execution(format!(
                "DuckDuckGo search returned status {}",
                resp.status()
            )));
        }

        let html = resp.text().await.map_err(|e| {
            ToolError::Execution(format!("Failed to read DuckDuckGo response: {e}"))
        })?;

        Ok(parse_duckduckgo_html(&html, max_results))
    }

    async fn search_brave(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResultItem>, ToolError> {
        let api_key = self
            .api_key
            .clone()
            .or_else(|| std::env::var("BRAVE_API_KEY").ok())
            .ok_or_else(|| {
                ToolError::Execution(
                    "Brave Search selected but no API key configured (BRAVE_API_KEY).".into(),
                )
            })?;

        let url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
            urlencoding_encode(query),
            max_results.clamp(1, 20)
        );

        let resp = self
            .client
            .get(&url)
            .header("X-Subscription-Token", api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("Brave search request failed: {e}")))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(ToolError::Execution(format!("Brave search error: {err}")));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to parse Brave JSON: {e}")))?;

        let mut results = Vec::new();
        if let Some(items) = json_val.pointer("/web/results").and_then(|v| v.as_array()) {
            for item in items.iter().take(max_results) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let link = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let snippet = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !title.is_empty() && !link.is_empty() {
                    results.push(SearchResultItem {
                        title,
                        url: link,
                        snippet,
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_tavily(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResultItem>, ToolError> {
        let api_key = self
            .api_key
            .clone()
            .or_else(|| std::env::var("TAVILY_API_KEY").ok())
            .ok_or_else(|| {
                ToolError::Execution(
                    "Tavily search selected but no API key configured (TAVILY_API_KEY).".into(),
                )
            })?;

        let body = json!({
            "api_key": api_key,
            "query": query,
            "max_results": max_results.clamp(1, 20),
            "search_depth": "basic",
        });

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("Tavily search request failed: {e}")))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(ToolError::Execution(format!("Tavily search error: {err}")));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to parse Tavily JSON: {e}")))?;

        let mut results = Vec::new();
        if let Some(items) = json_val.get("results").and_then(|v| v.as_array()) {
            for item in items.iter().take(max_results) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let link = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let snippet = item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !title.is_empty() && !link.is_empty() {
                    results.push(SearchResultItem {
                        title,
                        url: link,
                        snippet,
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_searxng(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResultItem>, ToolError> {
        let endpoint = self
            .endpoint
            .clone()
            .or_else(|| std::env::var("SEARXNG_URL").ok())
            .unwrap_or_else(|| "https://searx.be".to_string());

        let url = format!(
            "{}/search?q={}&format=json",
            endpoint.trim_end_matches('/'),
            urlencoding_encode(query)
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("SearXNG search request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ToolError::Execution(format!(
                "SearXNG returned status {}",
                resp.status()
            )));
        }

        let json_val: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to parse SearXNG JSON: {e}")))?;

        let mut results = Vec::new();
        if let Some(items) = json_val.get("results").and_then(|v| v.as_array()) {
            for item in items.iter().take(max_results) {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let link = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let snippet = item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !title.is_empty() && !link.is_empty() {
                    results.push(SearchResultItem {
                        title,
                        url: link,
                        snippet,
                    });
                }
            }
        }

        Ok(results)
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for up-to-date documentation, API references, library versions, and technical solutions."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query string (e.g. 'tokio broadcast channel rust documentation')"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of search results to return (default: 5, max: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters("Missing 'query' parameter".into()))?
            .trim();

        if query.is_empty() {
            return Err(ToolError::InvalidParameters(
                "Search query cannot be empty".into(),
            ));
        }

        let max_results = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;
        let max_results = max_results.clamp(1, 10);

        let results = match self.provider.as_str() {
            "brave" => self.search_brave(query, max_results).await?,
            "tavily" => self.search_tavily(query, max_results).await?,
            "searxng" => self.search_searxng(query, max_results).await?,
            _ => self.search_duckduckgo(query, max_results).await?,
        };

        if results.is_empty() {
            return Ok(format!("No search results found for query: \"{query}\""));
        }

        let mut out = format!("### Web Search Results for: \"{query}\"\n\n");
        for (i, item) in results.iter().enumerate() {
            out.push_str(&format!("{}. [{}]({})\n", i + 1, item.title, item.url));
            if !item.snippet.is_empty() {
                out.push_str(&format!("   {}\n", item.snippet.trim()));
            }
            out.push('\n');
        }

        Ok(out.trim_end().to_string())
    }
}

/// Addresses a fetched URL must never reach: link-local ranges (cloud metadata services such as
/// 169.254.169.254, `fe80::/10`), well-known metadata addresses outside them, and unspecified or
/// broadcast addresses. Other private addresses stay reachable so local docs servers still work.
fn is_forbidden_fetch_ip(ip: IpAddr) -> bool {
    const ALIBABA_METADATA: Ipv4Addr = Ipv4Addr::new(100, 100, 100, 200);
    const AWS_METADATA_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254);
    match ip {
        IpAddr::V4(v4) => {
            v4.is_link_local() || v4.is_unspecified() || v4.is_broadcast() || v4 == ALIBABA_METADATA
        }
        IpAddr::V6(v6) => {
            v6.is_unspecified()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6 == AWS_METADATA_V6
                || v6.to_ipv4_mapped().is_some_and(|v4| is_forbidden_fetch_ip(IpAddr::V4(v4)))
        }
    }
}

/// Why `url` must not be fetched, if it must not. Besides the addresses above, this blocks the
/// local MauKode daemon API: instructions on a fetched page could otherwise make the agent read
/// other sessions through it.
fn fetch_block_reason(url: &reqwest::Url) -> Option<String> {
    let host = url.host_str()?;
    let literal_ip = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .ok();
    if let Some(ip) = literal_ip.filter(|ip| is_forbidden_fetch_ip(*ip)) {
        return Some(format!("{ip} is a link-local or metadata address"));
    }
    let loopback = literal_ip.map_or_else(
        || host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost"),
        |ip| ip.is_loopback(),
    );
    if loopback && url.path().starts_with("/api/") {
        return Some("the local MauKode daemon API is not reachable from this tool".to_string());
    }
    None
}

/// DNS resolution that drops forbidden addresses, so a host name (or a redirect to one) cannot be
/// pointed at a metadata service either.
struct GuardedResolver;

impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(resolve_allowed(name.as_str().to_string()))
    }
}

async fn resolve_allowed(
    host: String,
) -> Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> {
    let allowed: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
        .await?
        .filter(|addr| !is_forbidden_fetch_ip(addr.ip()))
        .collect();
    if allowed.is_empty() {
        return Err(format!("{host} resolves only to blocked addresses").into());
    }
    Ok(Box::new(allowed.into_iter()))
}

/// Fetch web content tool for downloading web pages and converting HTML to clean Markdown
pub struct FetchWebContentTool {
    client: reqwest::Client,
    max_download_bytes: usize,
}

impl Default for FetchWebContentTool {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if let Some(reason) = fetch_block_reason(attempt.url()) {
                    attempt.error(reason)
                } else {
                    attempt.follow()
                }
            }))
            .dns_resolver(Arc::new(GuardedResolver))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            max_download_bytes: 2 * 1024 * 1024, // 2MB limit
        }
    }
}

impl FetchWebContentTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Tool for FetchWebContentTool {
    fn name(&self) -> &str {
        "fetch_web_content"
    }

    fn description(&self) -> &str {
        "Fetch a web page or online documentation URL and convert its content into clean, token-efficient Markdown."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The HTTP or HTTPS URL to fetch (e.g. 'https://docs.rs/tokio/latest/tokio/')"
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum character length of the returned markdown (default: 25000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters("Missing 'url' parameter".into()))?
            .trim();

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::InvalidParameters(
                "Invalid URL scheme: must start with 'http://' or 'https://'".into(),
            ));
        }

        let max_chars = args
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(25000) as usize;

        let parsed = reqwest::Url::parse(url)
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid URL '{url}': {e}")))?;
        if let Some(reason) = fetch_block_reason(&parsed) {
            return Err(ToolError::PermissionDenied(format!(
                "Refusing to fetch '{url}': {reason}"
            )));
        }

        let mut resp = self
            .client
            .get(url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/json,text/plain;q=0.9,*/*;q=0.8",
            )
            .send()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to fetch URL '{url}': {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(ToolError::Execution(format!(
                "HTTP request to '{url}' failed with status {status}"
            )));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();

        // Stop reading at the limit instead of buffering whatever the server sends.
        let too_large = || {
            ToolError::Execution(format!(
                "Content exceeds maximum download limit of {} bytes",
                self.max_download_bytes
            ))
        };
        if resp
            .content_length()
            .is_some_and(|len| len > self.max_download_bytes as u64)
        {
            return Err(too_large());
        }
        let mut raw_bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| ToolError::Execution(format!("Failed to read response body: {e}")))?
        {
            if raw_bytes.len() + chunk.len() > self.max_download_bytes {
                return Err(too_large());
            }
            raw_bytes.extend_from_slice(&chunk);
        }

        let body = String::from_utf8_lossy(&raw_bytes).to_string();

        let markdown = if content_type.contains("application/json") {
            if let Ok(json_parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                format!(
                    "```json\n{}\n```",
                    serde_json::to_string_pretty(&json_parsed).unwrap_or(body)
                )
            } else {
                format!("```json\n{body}\n```")
            }
        } else if content_type.contains("text/plain") {
            body
        } else {
            // HTML to clean Markdown
            html_to_clean_markdown(&body, url)
        };

        if markdown.len() > max_chars {
            let truncated = &markdown[..floor_char_boundary(&markdown, max_chars)];
            Ok(format!(
                "{truncated}\n\n[... Truncated: content exceeded {max_chars} characters. Original URL: {url} ...]"
            ))
        } else {
            Ok(markdown)
        }
    }
}

/// Convert raw HTML into clean, token-efficient Markdown
pub fn html_to_clean_markdown(html: &str, url: &str) -> String {
    // 1. Extract <title> if present
    let title_re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap();
    let title = title_re
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| decode_html_entities(m.as_str().trim()))
        .unwrap_or_default();

    // 2. Strip unwanted sections: <head>, <script>, <style>, <noscript>, <svg>, <nav>, <footer>, <header>, <aside>
    let strip_patterns = [
        r"(?is)<head[^>]*>.*?</head>",
        r"(?is)<script[^>]*>.*?</script>",
        r"(?is)<style[^>]*>.*?</style>",
        r"(?is)<noscript[^>]*>.*?</noscript>",
        r"(?is)<svg[^>]*>.*?</svg>",
        r"(?is)<canvas[^>]*>.*?</canvas>",
        r"(?is)<header[^>]*>.*?</header>",
        r"(?is)<footer[^>]*>.*?</footer>",
        r"(?is)<nav[^>]*>.*?</nav>",
        r"(?is)<aside[^>]*>.*?</aside>",
        r"(?is)<!--.*?-->",
    ];

    let mut text = html.to_string();
    for pattern in &strip_patterns {
        if let Ok(re) = Regex::new(pattern) {
            text = re.replace_all(&text, "").to_string();
        }
    }

    // 3. Convert code blocks (<pre><code>...</code></pre> and <pre>...</pre>)
    let pre_code_re = Regex::new(
        r"(?is)<pre[^>]*><code(?: class=[^>]*language-([a-zA-Z0-9_-]+)[^>]*)?>(.*?)</code></pre>",
    )
    .unwrap();
    text = pre_code_re
        .replace_all(&text, |caps: &regex::Captures| {
            let lang = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code = decode_html_entities(caps.get(2).map(|m| m.as_str()).unwrap_or(""));
            format!("\n\n```{lang}\n{code}\n```\n\n")
        })
        .to_string();

    let pre_re = Regex::new(r"(?is)<pre[^>]*>(.*?)</pre>").unwrap();
    text = pre_re
        .replace_all(&text, |caps: &regex::Captures| {
            let code = decode_html_entities(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
            format!("\n\n```\n{code}\n```\n\n")
        })
        .to_string();

    // 4. Convert inline code
    let code_re = Regex::new(r"(?is)<code[^>]*>(.*?)</code>").unwrap();
    text = code_re
        .replace_all(&text, |caps: &regex::Captures| {
            let code = decode_html_entities(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
            format!("`{code}`")
        })
        .to_string();

    // 5. Convert headings <h1> - <h6>
    for i in 1..=6 {
        let h_re = Regex::new(&format!(r"(?is)<h{i}[^>]*>(.*?)</h{i}>")).unwrap();
        let hashes = "#".repeat(i);
        text = h_re
            .replace_all(&text, |caps: &regex::Captures| {
                let inner = strip_html_tags(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
                format!("\n\n{hashes} {}\n\n", decode_html_entities(&inner).trim())
            })
            .to_string();
    }

    // 6. Convert links <a href="URL">TEXT</a>
    let a_re = Regex::new(r#"(?is)<a [^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap();
    text = a_re
        .replace_all(&text, |caps: &regex::Captures| {
            let href = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let label = strip_html_tags(caps.get(2).map(|m| m.as_str()).unwrap_or(""));
            let label_decoded = decode_html_entities(&label).trim().to_string();

            if label_decoded.is_empty()
                || href.starts_with('#')
                || href.starts_with("javascript:")
                || label_decoded.len() > 100
            {
                label_decoded
            } else {
                format!("[{label_decoded}]({href})")
            }
        })
        .to_string();

    // 7. Convert list items <li>
    let li_re = Regex::new(r"(?is)<li[^>]*>(.*?)</li>").unwrap();
    text = li_re
        .replace_all(&text, |caps: &regex::Captures| {
            let inner = strip_html_tags(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
            format!("\n- {}", decode_html_entities(&inner).trim())
        })
        .to_string();

    // 8. Convert paragraphs <p> and breaks <br>
    let p_re = Regex::new(r"(?is)<p[^>]*>(.*?)</p>").unwrap();
    text = p_re
        .replace_all(&text, |caps: &regex::Captures| {
            let inner = strip_html_tags(caps.get(1).map(|m| m.as_str()).unwrap_or(""));
            format!("\n\n{}\n\n", decode_html_entities(&inner).trim())
        })
        .to_string();

    let br_re = Regex::new(r"(?is)<br\s*/?>").unwrap();
    text = br_re.replace_all(&text, "\n").to_string();

    // 9. Strip any remaining HTML tags
    text = strip_html_tags(&text);
    text = decode_html_entities(&text);

    // 10. Clean and normalize whitespace
    let mut clean_lines = Vec::new();
    let mut empty_streak = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            empty_streak += 1;
            if empty_streak <= 1 {
                clean_lines.push("");
            }
        } else {
            empty_streak = 0;
            clean_lines.push(trimmed);
        }
    }

    let body_markdown = clean_lines.join("\n").trim().to_string();

    let mut output = String::new();
    if !title.is_empty() {
        output.push_str(&format!("# {title}\n"));
    }
    output.push_str(&format!("**Source URL:** {url}\n\n"));
    output.push_str(&body_markdown);

    output
}

/// Helper to strip HTML tags while leaving inner text
fn strip_html_tags(input: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(input, " ").to_string()
}

/// Helper to decode HTML entities
fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
}

/// Simple URL percent-encoding for query string
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// Parse DuckDuckGo HTML output to extract titles, urls, and snippets
fn parse_duckduckgo_html(html: &str, max_results: usize) -> Vec<SearchResultItem> {
    let mut results = Vec::new();

    // Regex matching result blocks in DuckDuckGo HTML
    // Class names in DDG html: result__title, result__url, result__snippet
    let result_block_re = Regex::new(r#"(?is)<div class="result[^"]*results_links[^"]*">.*?<a class="result__url"[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>.*?<a class="result__snippet"[^>]*>(.*?)</a>"#).unwrap();

    for caps in result_block_re.captures_iter(html) {
        if results.len() >= max_results {
            break;
        }

        let raw_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let title_raw = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let snippet_raw = caps.get(3).map(|m| m.as_str()).unwrap_or("");

        let real_url = extract_ddg_real_url(raw_url);
        let title = decode_html_entities(&strip_html_tags(title_raw))
            .trim()
            .to_string();
        let snippet = decode_html_entities(&strip_html_tags(snippet_raw))
            .trim()
            .to_string();

        if !title.is_empty() && !real_url.is_empty() {
            results.push(SearchResultItem {
                title,
                url: real_url,
                snippet,
            });
        }
    }

    // Fallback regex matching simpler anchor structures if classes differed
    if results.is_empty() {
        let fallback_re = Regex::new(r#"(?is)<a rel="nofollow" class="[^"]*result__snippet[^"]*"[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap();
        for caps in fallback_re.captures_iter(html) {
            if results.len() >= max_results {
                break;
            }
            let raw_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let snippet = decode_html_entities(&strip_html_tags(
                caps.get(2).map(|m| m.as_str()).unwrap_or(""),
            ))
            .trim()
            .to_string();
            let real_url = extract_ddg_real_url(raw_url);
            if !real_url.is_empty() {
                results.push(SearchResultItem {
                    title: real_url.clone(),
                    url: real_url,
                    snippet,
                });
            }
        }
    }

    results
}

/// Extract actual target URL from DuckDuckGo redirect link: //duckduckgo.com/l/?uddg=ENCODED_URL...
fn extract_ddg_real_url(raw: &str) -> String {
    if let Some(idx) = raw.find("uddg=") {
        let encoded = &raw[idx + 5..];
        let end_idx = encoded.find('&').unwrap_or(encoded.len());
        let target = &encoded[..end_idx];
        urlencoding_decode(target)
    } else if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    }
}

/// Simple percent-decoding helper
fn urlencoding_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                if let Ok(byte) = u8::from_str_radix(&format!("{c1}{c2}"), 16) {
                    bytes.push(byte);
                    continue;
                }
            }
        } else if c == '+' {
            bytes.push(b' ');
        } else {
            bytes.push(c as u8);
        }
    }

    String::from_utf8_lossy(&bytes).to_string()
}

fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_clean_markdown_strips_scripts_and_converts_tags() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head><title>Tokio Broadcast Documentation</title></head>
            <body>
                <header><nav><a href="/home">Home</a></nav></header>
                <h1>Tokio Broadcast Channel</h1>
                <p>A multi-producer, multi-consumer broadcast queue.</p>
                <script>alert("malicious script");</script>
                <style>body { color: red; }</style>
                <h2>Quick Example</h2>
                <pre><code class="language-rust">use tokio::sync::broadcast;
let (tx, mut rx1) = broadcast::channel(16);
</code></pre>
                <ul>
                    <li>Capacity is bounded</li>
                    <li>Lagging receivers receive Lagged error</li>
                </ul>
                <footer><p>© 2026 Tokio Contributors</p></footer>
            </body>
            </html>
        "#;

        let md = html_to_clean_markdown(html, "https://docs.rs/tokio/latest/tokio/sync/broadcast/");
        assert!(md.contains("# Tokio Broadcast Documentation"));
        assert!(md.contains("# Tokio Broadcast Channel"));
        assert!(md.contains("A multi-producer, multi-consumer broadcast queue."));
        assert!(md.contains("```rust"));
        assert!(md.contains("use tokio::sync::broadcast;"));
        assert!(md.contains("- Capacity is bounded"));
        // Script, styles, and nav must be completely stripped!
        assert!(!md.contains("malicious script"));
        assert!(!md.contains("color: red"));
        assert!(!md.contains("Home"));
        assert!(!md.contains("Tokio Contributors"));
    }

    #[test]
    fn test_ddg_real_url_extraction() {
        let ddg_redirect = "//duckduckgo.com/l/?kh=-1&uddg=https%3A%2F%2Fdocs.rs%2Ftokio%2Flatest%2Ftokio%2F&rut=...";
        let real = extract_ddg_real_url(ddg_redirect);
        assert_eq!(real, "https://docs.rs/tokio/latest/tokio/");
    }

    #[test]
    fn test_parse_duckduckgo_html_mock() {
        let html = r#"
            <div class="result results_links results_links_deep web-result">
                <div class="result__body links_main links_deep">
                    <h2 class="result__title">
                        <a class="result__url" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Ftokio-rs%2Ftokio">Tokio GitHub Repository</a>
                    </h2>
                    <a class="result__snippet">A runtime for writing reliable network applications with Rust.</a>
                </div>
            </div>
        "#;

        let results = parse_duckduckgo_html(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Tokio GitHub Repository");
        assert_eq!(results[0].url, "https://github.com/tokio-rs/tokio");
        assert!(results[0]
            .snippet
            .contains("runtime for writing reliable network applications"));
    }

    #[test]
    fn fetch_refuses_metadata_addresses_and_the_daemon_api() {
        let blocked = |url: &str| fetch_block_reason(&reqwest::Url::parse(url).unwrap()).is_some();
        assert!(blocked("http://169.254.169.254/latest/meta-data/"));
        assert!(blocked("http://[fe80::1]/"));
        assert!(blocked("http://[fd00:ec2::254]/"));
        assert!(blocked("http://100.100.100.200/"));
        assert!(blocked("http://0.0.0.0:8080/"));
        assert!(blocked("http://[::ffff:169.254.169.254]/"));
        assert!(blocked("http://127.0.0.1:4096/api/v1/sessions"));
        assert!(blocked("http://localhost:4096/api/v1/config"));
        assert!(!blocked("https://docs.rs/tokio/latest/tokio/"));
        assert!(!blocked("http://localhost:3000/docs"));
        assert!(!blocked("http://192.168.1.10/"));
    }
}
