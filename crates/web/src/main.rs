//! `painless-ghicon-web`: an Axum + HTMX front end for rounding the corners
//! of GitHub identicon block patterns.

mod app;
mod avatar;
mod templates;

use anyhow::Context as _;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Registry};

const DEFAULT_PORT: u16 = 8080;
const SERVICE_NAME: &str = "painless-ghicon-web";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tracer_provider = init_tracing();

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = format!("0.0.0.0:{port}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!("listening on {addr}");

    axum::serve(listener, app::app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    if let Some(provider) = tracer_provider
        && let Err(err) = provider.shutdown()
    {
        tracing::warn!("failed to shut down OpenTelemetry tracer provider: {err}");
    }

    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::warn!("failed to install Ctrl+C handler: {err}");
    }
}

/// The OpenTelemetry tracing layer, parameterized over the bare [`Registry`]
/// subscriber it is applied to. It must be composed onto the registry
/// *before* any other layer (see [`init_tracing`]) so this concrete `S`
/// matches what the type checker sees at that point in the chain.
type OtelLayer = tracing_opentelemetry::OpenTelemetryLayer<Registry, SdkTracer>;

/// Initializes tracing: an `EnvFilter` (falling back to `info`) and an fmt
/// layer are always installed. When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, an
/// OTLP/gRPC OpenTelemetry layer is also installed; failures to do so are
/// logged and degrade gracefully to fmt-only tracing.
///
/// Returns the tracer provider so the caller can shut it down on exit; `None`
/// when OpenTelemetry wasn't installed.
fn init_tracing() -> Option<SdkTracerProvider> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer();

    let (otel_layer, tracer_provider): (Option<OtelLayer>, Option<SdkTracerProvider>) =
        match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            Ok(_) => match build_otel_layer() {
                Ok((layer, provider)) => (Some(layer), Some(provider)),
                Err(err) => {
                    eprintln!("failed to initialize OpenTelemetry, continuing without it: {err}");
                    (None, None)
                }
            },
            Err(_) => (None, None),
        };

    tracing_subscriber::registry()
        .with(otel_layer)
        .with(env_filter)
        .with(fmt_layer)
        .init();

    tracer_provider
}

fn build_otel_layer() -> anyhow::Result<(OtelLayer, SdkTracerProvider)> {
    let exporter = SpanExporter::builder()
        .with_tonic()
        .build()
        .context("failed to build OTLP span exporter")?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    let tracer = provider.tracer(SERVICE_NAME);
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);
    Ok((layer, provider))
}
