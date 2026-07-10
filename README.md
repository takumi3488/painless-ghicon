# painless-ghicon

Rounds the corners of the block pattern in GitHub's default avatars (identicons) — the square canvas stays square, only the blocky shapes get smooth.

It auto-detects the two colors and the cell size of an identicon-like image, then applies morphological corner rounding (closing + opening via exact euclidean distance transforms) with anti-aliased edges. Both convex and concave corners are rounded; at the maximum radius, isolated cells become circles.

| before | after (`--ratio 0.4`) |
|--------|-----------------------|
| ![before](samples/takumi3488.png) | ![after](samples/takumi3488-rounded.png) |

## Workspace

| Crate | Path | Purpose |
|-------|------|---------|
| `painless-ghicon-core` | `crates/core` | Image binary conversion (decode → round pattern corners → PNG) |
| `painless-ghicon` | `crates/cli` | CLI: converts a local image or a GitHub user's avatar |
| `painless-ghicon-web` | `crates/web` | Axum + HTMX web app: upload an image or enter a GitHub ID/URL, download the result |

## CLI

```console
# from a local image
painless-ghicon path/to/icon.png

# from a GitHub user (ID or URL)
painless-ghicon octocat
painless-ghicon https://github.com/octocat

# options: corner radius as a fraction of the block size (0.0..=0.5], output path
painless-ghicon octocat --ratio 0.5 --output circle.png
```

## Web

```console
docker compose up --build
# open http://localhost:8080
```

Environment variables:

- `PORT` — listen port (default `8080`)
- `OTEL_EXPORTER_OTLP_ENDPOINT` — when set, traces are exported via OTLP gRPC (the compose file wires this to Jaeger, UI at http://localhost:16686)

## Development

```console
cargo test --workspace
cargo clippy --workspace --all-targets
```

