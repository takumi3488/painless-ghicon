//! CLI that rounds the corners of `GitHub` identicon block patterns.
//!
//! Accepts a local PNG/JPEG file, a `GitHub` user ID, or a `GitHub` user URL,
//! rounds the corners of the detected two-color block pattern, and writes
//! the result as a PNG.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use painless_ghicon_core::{DEFAULT_RADIUS_RATIO, resolve_avatar_url, round_image_bytes};

/// Maximum number of bytes that will be read from a remote avatar response.
const MAX_AVATAR_BYTES: u64 = 20 * 1024 * 1024;
/// `User-Agent` header sent with avatar download requests.
const USER_AGENT: &str = "painless-ghicon";
/// Timeout applied to avatar download requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Rounds the corners of a `GitHub` identicon block pattern.
#[derive(Debug, Parser)]
#[command(
    name = "painless-ghicon",
    version,
    about = "Rounds the corners of GitHub identicon block patterns."
)]
struct Cli {
    /// Path to a local PNG/JPEG file, a GitHub user ID, or a GitHub user URL.
    #[arg(help = "Path to a local PNG/JPEG file, a GitHub user ID, or a GitHub user URL.")]
    source: String,

    /// Corner radius as a fraction of the detected block size.
    #[arg(
        short,
        long,
        default_value_t = DEFAULT_RADIUS_RATIO,
        help = "Corner radius as a fraction of the detected block size, in (0.0, 0.5] (0.5 = full circles)."
    )]
    ratio: f32,

    /// Output PNG path.
    #[arg(
        short,
        long,
        help = "Output PNG path. Defaults to a name derived from the source."
    )]
    output: Option<PathBuf>,
}

/// How a CLI source string should be treated.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceKind {
    /// An existing local file at this path.
    File(PathBuf),
    /// A `GitHub` user ID or URL that must be resolved and downloaded.
    GitHub,
}

fn main() -> Result<()> {
    run(Cli::parse())
}

fn run(cli: Cli) -> Result<()> {
    let Cli {
        source,
        ratio,
        output,
    } = cli;

    let (bytes, default_output) = match classify_source(&source) {
        SourceKind::File(path) => {
            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read input file {}", path.display()))?;
            let default_output = default_output_path_for_file(&path);
            (bytes, default_output)
        }
        SourceKind::GitHub => {
            let url = resolve_avatar_url(&source)
                .with_context(|| format!("cannot resolve {source:?} as a GitHub avatar source"))?;
            let bytes = fetch_avatar(&url)?;
            let default_output = default_output_path_for_github(&url);
            (bytes, default_output)
        }
    };

    let output = output.unwrap_or(default_output);

    let rounded = round_image_bytes(&bytes, ratio).context("failed to round image")?;
    if !rounded.pattern_detected {
        eprintln!(
            "warning: no two-color block pattern detected in the input image; the output was written unchanged"
        );
    }

    fs::write(&output, &rounded.png)
        .with_context(|| format!("failed to write output file {}", output.display()))?;

    println!("{}", output.display());
    Ok(())
}

/// Classifies `source` as a local file (when it exists on disk) or as a
/// `GitHub` reference to be resolved and downloaded.
fn classify_source(source: &str) -> SourceKind {
    let path = Path::new(source);
    if path.is_file() {
        SourceKind::File(path.to_path_buf())
    } else {
        SourceKind::GitHub
    }
}

/// Derives the default output path for a local file input: a sibling file
/// named `<stem>-rounded.png` next to the input.
fn default_output_path_for_file(path: &Path) -> PathBuf {
    let stem = path.file_stem().map_or_else(
        || "output".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let file_name = format!("{stem}-rounded.png");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(file_name),
        _ => PathBuf::from(file_name),
    }
}

/// Derives the default output path for a `GitHub` avatar input from the
/// resolved avatar URL: `./<username>-rounded.png`. Avatar-host URLs (which
/// carry no username) fall back to `avatar`.
fn default_output_path_for_github(resolved_url: &str) -> PathBuf {
    let username = github_username_from_url(resolved_url).unwrap_or("avatar");
    PathBuf::from(format!("{username}-rounded.png"))
}

/// Extracts the `<name>` segment from a resolved `https://github.com/<name>.png`
/// URL, or `None` when the URL does not follow that shape (e.g. an
/// `avatars.githubusercontent.com` URL).
fn github_username_from_url(url: &str) -> Option<&str> {
    url.strip_prefix("https://github.com/")
        .and_then(|rest| rest.strip_suffix(".png"))
}

/// Downloads the avatar at `url`, enforcing a request timeout and a maximum
/// response size.
fn fetch_avatar(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to fetch {url}"))?;

    if !response.status().is_success() {
        bail!(
            "failed to fetch {url}: server responded with {}",
            response.status()
        );
    }

    if let Some(len) = response.content_length()
        && len > MAX_AVATAR_BYTES
    {
        bail!("avatar at {url} is {len} bytes, which exceeds the {MAX_AVATAR_BYTES}-byte limit");
    }

    let mut bytes = Vec::new();
    response
        .take(MAX_AVATAR_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read response body from {url}"))?;

    let actual_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual_len > MAX_AVATAR_BYTES {
        bail!("avatar at {url} exceeds the {MAX_AVATAR_BYTES}-byte limit");
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        SourceKind, classify_source, default_output_path_for_file, default_output_path_for_github,
        github_username_from_url,
    };
    use std::path::PathBuf;

    #[test]
    fn default_output_for_file_uses_sibling_rounded_name() {
        let path = PathBuf::from("inputs/octocat.png");
        assert_eq!(
            default_output_path_for_file(&path),
            PathBuf::from("inputs/octocat-rounded.png")
        );
    }

    #[test]
    fn default_output_for_file_without_parent_uses_cwd_relative_name() {
        let path = PathBuf::from("octocat.png");
        assert_eq!(
            default_output_path_for_file(&path),
            PathBuf::from("octocat-rounded.png")
        );
    }

    #[test]
    fn default_output_for_file_without_extension_still_appends_suffix() {
        let path = PathBuf::from("assets/octocat");
        assert_eq!(
            default_output_path_for_file(&path),
            PathBuf::from("assets/octocat-rounded.png")
        );
    }

    #[test]
    fn default_output_for_github_uses_username_from_profile_url() {
        assert_eq!(
            default_output_path_for_github("https://github.com/octocat.png"),
            PathBuf::from("octocat-rounded.png")
        );
    }

    #[test]
    fn default_output_for_github_falls_back_to_avatar_for_avatar_host() {
        assert_eq!(
            default_output_path_for_github("https://avatars.githubusercontent.com/u/1"),
            PathBuf::from("avatar-rounded.png")
        );
    }

    #[test]
    fn github_username_from_url_extracts_name() {
        let Some(name) = github_username_from_url("https://github.com/octocat.png") else {
            panic!("expected a username to be extracted");
        };
        assert_eq!(name, "octocat");
        assert!(github_username_from_url("https://avatars.githubusercontent.com/u/1").is_none());
    }

    #[test]
    fn classify_source_detects_existing_files() {
        let manifest_cargo_toml = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let SourceKind::File(path) = classify_source(manifest_cargo_toml) else {
            panic!("expected a File source for an existing path");
        };
        assert_eq!(path, PathBuf::from(manifest_cargo_toml));
    }

    #[test]
    fn classify_source_treats_non_existent_paths_as_github() {
        assert_eq!(classify_source("octocat"), SourceKind::GitHub);
        assert_eq!(
            classify_source("this-path-does-not-exist.png"),
            SourceKind::GitHub
        );
    }
}
