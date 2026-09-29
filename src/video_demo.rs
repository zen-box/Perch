//! 录制专用网络重定向。必须同时打开编译 feature 和启动环境变量。
//! 只接受官方 Claude 地址，避免误把用户的自定义渠道或密钥送进模拟服务。

use crate::config::ChannelType;
use url::Url;

const OFFICIAL_BASE: &str = "https://api.anthropic.com/v1";
const MOCK_BASE: &str = "http://127.0.0.1:18790/api.anthropic.com";

fn eligible(channel: ChannelType, base_url: &str, enabled: bool) -> bool {
    enabled && channel == ChannelType::Claude && base_url.trim() == OFFICIAL_BASE
}

pub fn active() -> bool {
    cfg!(feature = "video-demo") && std::env::var("PERCH_VIDEO_DEMO").is_ok_and(|value| value == "1")
}

pub fn is_official_claude(channel: ChannelType, base_url: &str) -> bool {
    eligible(channel, base_url, active())
}

fn redirect_url_for(url: &str, channel: ChannelType, base_url: &str, enabled: bool) -> Option<String> {
    if !eligible(channel, base_url, enabled) {
        return None;
    }
    let target = Url::parse(url).ok()?;
    if target.scheme() != "https"
        || target.host_str() != Some("api.anthropic.com")
        || target.port().is_some()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
        || !matches!(target.path(), "/v1/messages" | "/v1/models")
    {
        return None;
    }
    Some(format!("{MOCK_BASE}{}", target.path()))
}

pub fn redirect_url(url: &str, channel: ChannelType, base_url: &str) -> Option<String> {
    redirect_url_for(url, channel, base_url, is_official_claude(channel, base_url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_redirect_is_narrow_and_requires_both_switches() {
        assert!(!eligible(ChannelType::Claude, OFFICIAL_BASE, false));
        assert!(!eligible(ChannelType::Gemini, OFFICIAL_BASE, true));
        assert!(!eligible(ChannelType::Claude, "https://relay.example/v1", true));
        assert_eq!(
            redirect_url_for(
                "https://api.anthropic.com/v1/messages",
                ChannelType::Claude,
                OFFICIAL_BASE,
                true
            ),
            Some("http://127.0.0.1:18790/api.anthropic.com/v1/messages".into())
        );
        assert_eq!(
            redirect_url_for(
                "https://api.anthropic.com/v1/models",
                ChannelType::Claude,
                OFFICIAL_BASE,
                true
            ),
            Some("http://127.0.0.1:18790/api.anthropic.com/v1/models".into())
        );
        for url in [
            "https://relay.example/v1/messages",
            "https://api.anthropic.com/v1/other",
            "https://api.anthropic.com/v1/messages?key=secret",
            "http://api.anthropic.com/v1/messages",
        ] {
            assert_eq!(redirect_url_for(url, ChannelType::Claude, OFFICIAL_BASE, true), None);
        }
    }
}
