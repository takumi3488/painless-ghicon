//! HTML rendering for the index page and the `/convert` result fragments.

use axum::response::Html;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// The full index page. HTMX drives the `/convert` form without a page
/// reload; everything else is static markup, so no escaping is required.
const PAGE_HTML: &str = r##"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>painless-ghicon</title>
<script src="https://unpkg.com/htmx.org@2"></script>
<style>
  :root { color-scheme: light dark; }
  body {
    font-family: system-ui, -apple-system, "Hiragino Sans", sans-serif;
    background: #f4f4f5;
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .card {
    background: #fff;
    border-radius: 12px;
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.08);
    padding: 2rem;
    max-width: 480px;
    width: 100%;
  }
  h1 { font-size: 1.25rem; margin-top: 0; }
  p.description { color: #52525b; font-size: 0.9rem; }
  label { display: block; margin-top: 1rem; font-size: 0.85rem; color: #3f3f46; }
  input[type="file"], input[type="text"], input[type="number"] {
    width: 100%;
    box-sizing: border-box;
    padding: 0.5rem;
    margin-top: 0.25rem;
    border: 1px solid #d4d4d8;
    border-radius: 6px;
  }
  button {
    margin-top: 1.5rem;
    width: 100%;
    padding: 0.6rem;
    border: none;
    border-radius: 6px;
    background: #18181b;
    color: #fff;
    font-size: 1rem;
    cursor: pointer;
  }
  #result { margin-top: 1.5rem; }
  #result img { max-width: 100%; border-radius: 8px; }
  .error { color: #b91c1c; }
  .note { color: #92400e; font-size: 0.85rem; }
</style>
</head>
<body>
  <main class="card">
    <h1>painless-ghicon</h1>
    <p class="description">画像をアップロードするか、GitHubユーザーを指定してください。</p>
    <form hx-post="/convert" hx-target="#result" hx-encoding="multipart/form-data">
      <label>画像ファイル
        <input type="file" name="image" accept="image/png,image/jpeg">
      </label>
      <label>GitHub ID または URL
        <input type="text" name="github" placeholder="GitHub ID または URL">
      </label>
      <label>角丸の比率
        <input type="number" name="ratio" min="0.05" max="0.5" step="0.05" value="0.4">
      </label>
      <button type="submit">変換</button>
    </form>
    <div id="result"></div>
  </main>
</body>
</html>
"##;

/// Renders the static index page.
pub fn page() -> Html<&'static str> {
    Html(PAGE_HTML)
}

/// Renders an error fragment for `#result`. `message` is escaped, so callers
/// may pass raw, user-derived text.
pub fn error_fragment(message: &str) -> Html<String> {
    let message = html_escape::encode_text(message);
    Html(format!(r#"<p class="error">{message}</p>"#))
}

/// Renders the success fragment for `#result`: the rounded image, a download
/// link, and (when no pattern was detected) an explanatory note. `name` is
/// escaped, so callers may pass raw, user-derived text.
pub fn success_fragment(name: &str, png: &[u8], pattern_detected: bool) -> Html<String> {
    let encoded = STANDARD.encode(png);
    let safe_name = html_escape::encode_double_quoted_attribute(name);
    let note = if pattern_detected {
        String::new()
    } else {
        "<p class=\"note\">2色のブロック模様を検出できなかったため、元の画像をそのまま返しています。</p>"
            .to_string()
    };
    Html(format!(
        "<img src=\"data:image/png;base64,{encoded}\" alt=\"変換結果\">\
         <p><a href=\"data:image/png;base64,{encoded}\" download=\"{safe_name}-rounded.png\">ダウンロード</a></p>\
         {note}"
    ))
}
