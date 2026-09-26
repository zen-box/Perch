use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use gpui_kit::http_client::{self, AsyncBody, HttpClient, Request, Response};
use reqwest::Client;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::{Host, Url};

use crate::config::AppConfig;

const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// 统一图片地址的写法：只接受 http/https，去掉 `#` 片段，非 ASCII 字符转义。
/// 渲染与下载两边都用它比较，避免同一地址因写法不同而对不上。
pub fn normalize_image_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    url.set_fragment(None);
    Some(url.into())
}

/// 图片地址的主机名，用于在占位卡片上告诉用户图片来自哪里
pub fn image_host(url: &str) -> String {
    Url::parse(url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// 本机、局域网、链路本地等地址不允许作为图片来源
pub fn is_blocked_host(url: &Url) -> bool {
    // 仅调试版本：用本地模拟服务测试图片时放行内网地址
    if cfg!(debug_assertions) && std::env::var_os("PC_DEV_ALLOW_LOCAL_IMAGES").is_some() {
        return false;
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => is_private_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_private_ip(IpAddr::V6(ip)),
        Some(Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            domain == "localhost"
                || !domain.contains('.')
                || [".localhost", ".local", ".lan", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| domain.ends_with(suffix))
        }
        None => true,
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_private_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return is_private_v4(mapped);
            }
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (first & 0xfe00) == 0xfc00 // fc00::/7 唯一本地地址
                || (first & 0xffc0) == 0xfe80 // fe80::/10 链路本地地址
        }
    }
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    let [a, ..] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || a == 0
        || a >= 240
}

/// 解析域名时丢掉内网地址，防止公网域名（例如 127.0.0.1.nip.io）指向本机绕过检查。
struct PublicOnlyResolver {
    /// 代理常开在本机，解析代理自身的地址时不做限制
    proxy_host: Option<String>,
}

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let allow_private = self
            .proxy_host
            .as_deref()
            .is_some_and(|proxy| proxy.eq_ignore_ascii_case(&host));
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| allow_private || !is_private_ip(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} 指向本机或内网地址，已阻止加载").into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// GPUI 桌面端默认没有 HTTP 客户端，Markdown 里的 `![](https://...)` 会一直下载失败，
/// 图片区域就是空白。这里用已有的 reqwest 把用户同意加载的远程图片拉回来。
pub struct ImageHttpClient {
    client: Client,
    user_agent: http_client::http::HeaderValue,
}

impl ImageHttpClient {
    pub fn new(proxy: &str) -> Self {
        let proxy = proxy_url(proxy).and_then(|value| reqwest_proxy(&value).map(|proxy| (value, proxy)));
        let proxy_host = proxy.as_ref().and_then(|(value, _)| proxy_host(value));
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 8 {
                    attempt.error("图片重定向次数过多")
                } else if is_blocked_host(attempt.url()) {
                    attempt.error("图片重定向到了本机或内网地址，已阻止")
                } else {
                    attempt.follow()
                }
            }))
            .user_agent("Perch/0.1");

        if let Some((_, proxy)) = proxy {
            builder = builder.proxy(proxy);
        } else {
            builder = builder.dns_resolver(Arc::new(PublicOnlyResolver { proxy_host }));
        }

        let client = builder.build().unwrap_or_else(|_| Client::new());
        Self {
            client,
            user_agent: http_client::http::HeaderValue::from_static("Perch/0.1"),
        }
    }
}

pub fn client_for_config(config: &AppConfig) -> Arc<dyn HttpClient> {
    let proxy = config
        .providers
        .iter()
        .find(|provider| provider.id == config.active_provider_id)
        .map(|provider| provider.proxy.as_str())
        .unwrap_or("");
    Arc::new(ImageHttpClient::new(proxy))
}

fn proxy_url(explicit: &str) -> Option<String> {
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        return Some(explicit.to_string());
    }
    [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .into_iter()
    .find_map(|name| std::env::var(name).ok())
    .filter(|value| !value.trim().is_empty())
}

fn reqwest_proxy(value: &str) -> Option<reqwest::Proxy> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(proxy) = reqwest::Proxy::all(value) {
        return Some(proxy);
    }
    if !value.contains("://") {
        return reqwest::Proxy::all(format!("http://{value}")).ok();
    }
    None
}

fn proxy_host(value: &str) -> Option<String> {
    let value = value.trim();
    let with_scheme = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    Url::parse(&with_scheme).ok()?.host_str().map(str::to_string)
}

#[derive(Debug)]
struct ImageError(String);

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImageError {}

fn failure<T>(message: impl Into<String>) -> http_client::Result<T> {
    Err(ImageError(message.into()).into())
}

/// 下载前的检查：地址合法、不是内网地址
fn check_request(uri: &str) -> Result<(), String> {
    let Some(normalized) = normalize_image_url(uri) else {
        return Err(format!("只支持 http/https 图片: {uri}"));
    };
    let blocked = Url::parse(&normalized).map(|url| is_blocked_host(&url)).unwrap_or(true);
    if blocked {
        return Err("不加载本机或内网地址的图片".into());
    }
    Ok(())
}

fn image_cache_path(uri: &str) -> std::path::PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    let hash = hasher.finish();
    let cache_dir = crate::paths::data_dir().join("cache").join("images");
    // 建不出来也无所谓：下面写缓存会失败并被忽略，图片本身照样能显示
    let _ = std::fs::create_dir_all(&cache_dir);
    cache_dir.join(format!("{hash:016x}"))
}

impl HttpClient for ImageHttpClient {
    fn user_agent(&self) -> Option<&http_client::http::HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&http_client::Url> {
        None
    }

    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> futures::future::BoxFuture<'static, http_client::Result<Response<AsyncBody>>> {
        let client = self.client.clone();
        let method = req.method().as_str().to_string();
        let uri = req.uri().to_string();
        async move {
            if let Err(message) = check_request(&uri) {
                return failure(message);
            }
            let (tx, rx) = tokio::sync::oneshot::channel();
            crate::app::runtime().spawn(async move {
                let result = fetch(&client, &method, &uri).await;
                let _ = tx.send(result);
            });
            match rx.await {
                Ok(result) => result,
                Err(_) => failure("图片请求已取消"),
            }
        }
        .boxed()
    }
}

async fn fetch(client: &Client, method: &str, uri: &str) -> http_client::Result<Response<AsyncBody>> {
    let cache_file = image_cache_path(uri);
    if method.eq_ignore_ascii_case("GET")
        && let Ok(bytes) = std::fs::read(&cache_file)
        && !bytes.is_empty()
    {
        return http_client::http::Response::builder()
            .status(http_client::http::StatusCode::OK)
            .header(http_client::http::header::CONTENT_TYPE, "image/*")
            .body(AsyncBody::from(bytes))
            .map_err(|error| ImageError(error.to_string()).into());
    }

    let method_obj = reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
    let response = client
        .request(method_obj, uri)
        .header(
            reqwest::header::ACCEPT,
            "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
        )
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(|error| ImageError(format!("无法下载图片 {uri}: {error}")))?;
    let status = http_client::http::StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(http_client::http::StatusCode::BAD_GATEWAY);
    let headers = response.headers().clone();
    if let Some(length) = response.content_length()
        && length as usize > MAX_IMAGE_BYTES
    {
        return failure(format!("图片过大（超过 16MB）: {uri}"));
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| ImageError(format!("读取图片失败 {uri}: {error}")))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_IMAGE_BYTES {
            return failure(format!("图片过大（超过 16MB）: {uri}"));
        }
        body.extend_from_slice(&chunk);
    }

    if status.is_success() && method.eq_ignore_ascii_case("GET") && !body.is_empty() {
        // 缓存写失败不影响这次请求：图片已经在内存里了，下次重新下就是了
        let _ = crate::paths::write_atomic_bytes(&cache_file, &body);
    }

    let mut builder = http_client::http::Response::builder().status(status);
    for (name, value) in headers.iter() {
        let header_name = name.as_str();
        if header_name.eq_ignore_ascii_case("transfer-encoding")
            || header_name.eq_ignore_ascii_case("content-length")
            || header_name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            http_client::http::HeaderName::from_bytes(header_name.as_bytes()),
            http_client::http::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            builder = builder.header(name, value);
        }
    }
    builder
        .body(AsyncBody::from(body))
        .map_err(|error| ImageError(error.to_string()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(url: &str) -> bool {
        is_blocked_host(&Url::parse(url).unwrap())
    }

    #[test]
    fn only_http_urls_are_normalized() {
        assert_eq!(
            normalize_image_url("https://picsum.photos/200/100#top").as_deref(),
            Some("https://picsum.photos/200/100")
        );
        assert_eq!(
            normalize_image_url("HTTP://Example.com").as_deref(),
            Some("http://example.com/")
        );
        assert!(normalize_image_url("file:///C:/a.png").is_none());
        assert!(normalize_image_url("data:image/png;base64,aaaa").is_none());
        assert!(normalize_image_url("images/a.png").is_none());
    }

    #[test]
    fn valid_public_images_pass_check_request() {
        let url = "https://example.com/image.png";
        assert!(check_request(url).is_ok());
        assert!(check_request("not-a-url").is_err());
    }

    #[test]
    fn local_and_private_hosts_are_blocked() {
        assert!(blocked("http://127.0.0.1:18765/leak.png"));
        assert!(blocked("http://localhost/a.png"));
        assert!(blocked("http://app.localhost/a.png"));
        assert!(blocked("http://192.168.1.10/a.png"));
        assert!(blocked("http://10.0.0.1/a.png"));
        assert!(blocked("http://172.20.0.1/a.png"));
        assert!(blocked("http://169.254.169.254/latest/meta-data"));
        assert!(blocked("http://[::1]/a.png"));
        assert!(blocked("http://[fd00::1]/a.png"));
        assert!(blocked("http://[::ffff:127.0.0.1]/a.png"));
        assert!(blocked("http://2130706433/a.png"));
        assert!(blocked("http://nas/a.png"));
        assert!(blocked("http://printer.local/a.png"));
        assert!(!blocked("https://picsum.photos/200/100"));
        assert!(!blocked("https://8.8.8.8/a.png"));
    }

    /// 私网地址带查询串也一样拦掉：查询串不能把主机名解析绕过去
    #[test]
    fn private_address_with_query_is_rejected() {
        let url = "http://127.0.0.1:18765/leak.png?note=secret";
        assert!(check_request(url).is_err());
    }

    #[test]
    fn proxy_without_scheme_gets_http() {
        assert!(reqwest_proxy("127.0.0.1:7890").is_some());
        assert!(reqwest_proxy("http://127.0.0.1:7890").is_some());
        assert!(reqwest_proxy("").is_none());
        assert_eq!(proxy_host("127.0.0.1:7890").as_deref(), Some("127.0.0.1"));
        assert_eq!(proxy_host("socks5://localhost:1080").as_deref(), Some("localhost"));
    }
}
