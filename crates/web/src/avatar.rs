//! Fetching GitHub avatar images over a hardened HTTP client.

use std::sync::LazyLock;
use std::time::Duration;

const USER_AGENT: &str = "painless-ghicon-web";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_AVATAR_BYTES: usize = 20 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// Shared HTTP client used for all avatar fetches. Built lazily so that
/// construction failures (extremely unlikely, and impossible to `unwrap`
/// under this crate's lints) fall back to a plain default client instead of
/// panicking.
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(build_client);

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .redirect(redirect_policy())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Only follow redirects toward github.com or its avatar CDN, and only for a
/// bounded number of hops. `resolve_avatar_url` already restricts the initial
/// request to those hosts; this policy prevents a compromised or malicious
/// redirect from exfiltrating the request elsewhere (SSRF guard).
fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match attempt.url().host_str() {
            Some(host) if host == "github.com" || host.ends_with(".githubusercontent.com") => {
                attempt.follow()
            }
            _ => attempt.stop(),
        }
    })
}

/// Fetches the avatar bytes at `url`. Errors are already-friendly, Japanese
/// messages suitable for display to end users.
pub async fn fetch_avatar(url: &str) -> Result<Vec<u8>, String> {
    let response = CLIENT
        .get(url)
        .send()
        .await
        .map_err(|err| format!("アバターの取得に失敗しました: {err}"))?;

    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("ユーザーが見つかりませんでした".to_string());
    }
    if !status.is_success() {
        return Err(format!("アバターの取得に失敗しました (status {status})"));
    }
    if let Some(len) = response.content_length()
        && len > MAX_AVATAR_BYTES as u64
    {
        return Err("アバター画像のサイズが上限を超えています".to_string());
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("アバターの取得に失敗しました: {err}"))?;
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err("アバター画像のサイズが上限を超えています".to_string());
    }

    Ok(bytes.to_vec())
}

/// Derives a display name from a resolved avatar URL: the GitHub user name
/// for `https://github.com/{name}.png` URLs, otherwise a generic fallback.
pub fn display_name(url: &str) -> String {
    url.strip_prefix("https://github.com/")
        .and_then(|rest| rest.strip_suffix(".png"))
        .filter(|name| !name.is_empty())
        .map_or_else(|| "avatar".to_string(), str::to_string)
}
