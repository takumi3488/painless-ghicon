//! Route handlers and the [`Router`] they are wired into.

use axum::Router;
use axum::extract::{DefaultBodyLimit, Multipart};
use axum::response::Html;
use axum::routing::{get, post};
use painless_ghicon_core::DEFAULT_RADIUS_RATIO;
use std::path::Path;

use crate::{avatar, templates};

/// Requests bodies (mostly the uploaded image) are capped well above a
/// typical avatar/icon size, but far below anything that could be used to
/// exhaust memory.
const MAX_BODY_BYTES: usize = 20 * 1024 * 1024;

/// Builds the application router. Kept separate from `main` so tests can
/// drive it directly with `tower::ServiceExt::oneshot`.
pub fn app() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/convert", post(convert))
        .route("/healthz", get(healthz))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

async fn index() -> Html<&'static str> {
    templates::page()
}

async fn healthz() -> &'static str {
    "ok"
}

/// Parsed, not-yet-converted request: either uploaded image bytes with a
/// display name, or a GitHub avatar URL with a display name.
struct Source {
    bytes: Vec<u8>,
    name: String,
}

async fn convert(mut multipart: Multipart) -> Html<String> {
    let mut image_bytes: Vec<u8> = Vec::new();
    let mut image_filename: Option<String> = None;
    let mut github_input = String::new();
    let mut ratio_input = String::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => {
                return templates::error_fragment(&format!(
                    "フォームの読み取りに失敗しました: {err}"
                ));
            }
        };

        let field_name = field.name().unwrap_or_default().to_string();
        match field_name.as_str() {
            "image" => {
                let filename = field.file_name().map(str::to_string);
                match field.bytes().await {
                    Ok(bytes) => {
                        if !bytes.is_empty() {
                            image_bytes = bytes.to_vec();
                            image_filename = filename;
                        }
                    }
                    Err(err) => {
                        return templates::error_fragment(&format!(
                            "画像の読み込みに失敗しました: {err}"
                        ));
                    }
                }
            }
            "github" => match field.text().await {
                Ok(text) => github_input = text,
                Err(err) => {
                    return templates::error_fragment(&format!(
                        "GitHub入力の読み込みに失敗しました: {err}"
                    ));
                }
            },
            "ratio" => match field.text().await {
                Ok(text) => ratio_input = text,
                Err(err) => {
                    return templates::error_fragment(&format!(
                        "比率の読み込みに失敗しました: {err}"
                    ));
                }
            },
            _ => {
                if let Err(err) = field.bytes().await {
                    return templates::error_fragment(&format!(
                        "フォームの読み取りに失敗しました: {err}"
                    ));
                }
            }
        }
    }

    let ratio = if ratio_input.trim().is_empty() {
        DEFAULT_RADIUS_RATIO
    } else {
        match ratio_input.trim().parse::<f32>() {
            Ok(value) => value,
            Err(_) => {
                return templates::error_fragment("角丸の比率は数値で指定してください。");
            }
        }
    };

    let source = if image_bytes.is_empty() {
        if github_input.trim().is_empty() {
            return templates::error_fragment(
                "画像をアップロードするか、GitHubユーザーを指定してください。",
            );
        }
        match resolve_from_github(&github_input).await {
            Ok(source) => source,
            Err(message) => return templates::error_fragment(&message),
        }
    } else {
        let name = image_filename
            .as_deref()
            .map_or_else(|| "icon".to_string(), stem_or_fallback);
        Source {
            bytes: image_bytes,
            name,
        }
    };

    match painless_ghicon_core::round_image_bytes(&source.bytes, ratio) {
        Ok(rounded) => {
            templates::success_fragment(&source.name, &rounded.png, rounded.pattern_detected)
        }
        Err(err) => templates::error_fragment(&format!("変換に失敗しました: {err}")),
    }
}

async fn resolve_from_github(github_input: &str) -> Result<Source, String> {
    let avatar_url = painless_ghicon_core::resolve_avatar_url(github_input)
        .map_err(|err| format!("GitHubの入力を解決できませんでした: {err}"))?;
    let bytes = avatar::fetch_avatar(&avatar_url).await?;
    let name = avatar::display_name(&avatar_url);
    Ok(Source { bytes, name })
}

/// Derives a display name from an uploaded filename's stem, falling back to
/// `"icon"` when the stem is missing or empty.
fn stem_or_fallback(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map_or_else(|| "icon".to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::app;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use image::{Rgb, RgbImage};
    use std::io::Cursor;
    use tower::ServiceExt;

    fn identicon_png_bytes() -> Vec<u8> {
        let mut img = RgbImage::from_pixel(210, 210, Rgb([240, 240, 240]));
        for y in 70..140 {
            for x in 70..140 {
                img.put_pixel(x, y, Rgb([50, 100, 200]));
            }
        }
        let mut bytes = Vec::new();
        let Ok(()) = img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png) else {
            panic!("test identicon should encode as PNG");
        };
        bytes
    }

    async fn body_text(response: axum::response::Response) -> String {
        let Ok(collected) = response.into_body().collect().await else {
            panic!("response body should collect");
        };
        String::from_utf8_lossy(&collected.to_bytes()).into_owned()
    }

    /// `Router::call`'s error type is `Infallible`, so this can never
    /// actually panic; matching on the empty error type instead of an
    /// irrefutable `let Ok(..) else` keeps clippy/rustc happy about it.
    async fn oneshot(request: Request<Body>) -> axum::response::Response {
        match app().oneshot(request).await {
            Ok(response) => response,
            Err(err) => match err {},
        }
    }

    #[tokio::test]
    async fn index_page_serves_the_convert_form() {
        let Ok(request) = Request::builder().uri("/").body(Body::empty()) else {
            panic!("request should build");
        };
        let response = oneshot(request).await;
        assert_eq!(response.status(), 200);
        let text = body_text(response).await;
        assert!(text.contains("hx-post"));
        assert!(text.contains("/convert"));
    }

    #[tokio::test]
    async fn healthz_reports_ok() {
        let Ok(request) = Request::builder().uri("/healthz").body(Body::empty()) else {
            panic!("request should build");
        };
        let response = oneshot(request).await;
        assert_eq!(response.status(), 200);
        assert_eq!(body_text(response).await, "ok");
    }

    #[tokio::test]
    async fn convert_with_image_returns_a_data_url() {
        let boundary = "painless-ghicon-test-boundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\n\
                 Content-Disposition: form-data; name=\"image\"; filename=\"identicon.png\"\r\n\
                 Content-Type: image/png\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(&identicon_png_bytes());
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let Ok(request) = Request::builder()
            .method("POST")
            .uri("/convert")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
        else {
            panic!("request should build");
        };
        let response = oneshot(request).await;
        assert_eq!(response.status(), 200);
        let text = body_text(response).await;
        assert!(text.contains("data:image/png;base64"));
    }

    #[tokio::test]
    async fn convert_without_input_returns_an_error_fragment() {
        let boundary = "painless-ghicon-empty-boundary";
        let body = format!("--{boundary}--\r\n").into_bytes();

        let Ok(request) = Request::builder()
            .method("POST")
            .uri("/convert")
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
        else {
            panic!("request should build");
        };
        let response = oneshot(request).await;
        assert_eq!(response.status(), 200);
        let text = body_text(response).await;
        assert!(text.contains(r#"class="error""#));
    }
}
