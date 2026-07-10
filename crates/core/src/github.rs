//! Resolution of GitHub user IDs / URLs to avatar image URLs.

use crate::Error;

const GITHUB_HOSTS: [&str; 2] = ["github.com", "www.github.com"];
const AVATAR_HOST: &str = "avatars.githubusercontent.com";

/// Resolves a GitHub user ID or URL to the HTTPS URL of the user's avatar.
///
/// Accepted inputs:
/// - a bare user name (`octocat`)
/// - a profile URL (`https://github.com/octocat`, with or without `.png`)
/// - an avatar URL (`https://avatars.githubusercontent.com/...`), passed
///   through as-is
///
/// Everything else — in particular URLs pointing at other hosts — is
/// rejected, so callers can safely fetch the returned URL without further
/// validation.
///
/// # Errors
///
/// Returns [`Error::InvalidSource`] when the input is empty, is not a valid
/// GitHub user name, uses a scheme other than http(s), or points at a host
/// other than github.com / avatars.githubusercontent.com.
pub fn resolve_avatar_url(input: &str) -> Result<String, Error> {
    let input = input.trim();
    let invalid = |reason: &str| Error::InvalidSource {
        input: input.to_string(),
        reason: reason.to_string(),
    };

    if input.is_empty() {
        return Err(invalid("empty input"));
    }

    let Some((scheme, rest)) = input.split_once("://") else {
        if is_valid_username(input) {
            return Ok(format!("https://github.com/{input}.png"));
        }
        return Err(invalid("not a valid GitHub user name"));
    };

    if !matches!(scheme, "http" | "https") {
        return Err(invalid("only http(s) URLs are supported"));
    }
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.contains('@') || host.contains(':') {
        return Err(invalid("userinfo and explicit ports are not supported"));
    }
    let host = host.to_ascii_lowercase();
    if host == AVATAR_HOST {
        return Ok(format!("https://{rest}"));
    }
    if GITHUB_HOSTS.contains(&host.as_str()) {
        let first_segment = path.split(['/', '?', '#']).next().unwrap_or("");
        let name = first_segment.strip_suffix(".png").unwrap_or(first_segment);
        if is_valid_username(name) {
            return Ok(format!("https://github.com/{name}.png"));
        }
        return Err(invalid("URL does not contain a valid GitHub user name"));
    }
    Err(invalid(
        "host is not github.com or avatars.githubusercontent.com",
    ))
}

/// GitHub user names are 1–39 characters of ASCII alphanumerics and hyphens,
/// and cannot start or end with a hyphen.
fn is_valid_username(name: &str) -> bool {
    (1..=39).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::resolve_avatar_url;

    fn ok(input: &str) -> String {
        match resolve_avatar_url(input) {
            Ok(url) => url,
            Err(error) => panic!("{input:?} should resolve, got {error}"),
        }
    }

    #[test]
    fn bare_user_name_resolves_to_github_png() {
        assert_eq!(ok("octocat"), "https://github.com/octocat.png");
        assert_eq!(ok("  takumi3488  "), "https://github.com/takumi3488.png");
    }

    #[test]
    fn profile_urls_resolve_to_github_png() {
        assert_eq!(
            ok("https://github.com/octocat"),
            "https://github.com/octocat.png"
        );
        assert_eq!(
            ok("https://github.com/octocat.png"),
            "https://github.com/octocat.png"
        );
        assert_eq!(
            ok("https://www.github.com/octocat/"),
            "https://github.com/octocat.png"
        );
        assert_eq!(
            ok("http://github.com/octocat?tab=repos"),
            "https://github.com/octocat.png"
        );
    }

    #[test]
    fn avatar_urls_pass_through_as_https() {
        assert_eq!(
            ok("https://avatars.githubusercontent.com/u/12345?v=4"),
            "https://avatars.githubusercontent.com/u/12345?v=4"
        );
        assert_eq!(
            ok("http://avatars.githubusercontent.com/u/1"),
            "https://avatars.githubusercontent.com/u/1"
        );
    }

    #[test]
    fn other_hosts_and_schemes_are_rejected() {
        assert!(resolve_avatar_url("https://example.com/octocat.png").is_err());
        assert!(resolve_avatar_url("ftp://github.com/octocat").is_err());
        assert!(resolve_avatar_url("https://github.com@evil.com/octocat").is_err());
        assert!(resolve_avatar_url("https://github.com:8443/octocat").is_err());
    }

    #[test]
    fn invalid_user_names_are_rejected() {
        assert!(resolve_avatar_url("").is_err());
        assert!(resolve_avatar_url("-leading").is_err());
        assert!(resolve_avatar_url("trailing-").is_err());
        assert!(resolve_avatar_url("has space").is_err());
        assert!(resolve_avatar_url(&"a".repeat(40)).is_err());
        assert!(resolve_avatar_url("https://github.com/").is_err());
    }
}
