# Chef stage: Install cargo-chef
FROM lukemathwalker/cargo-chef:latest-rust-1.96.1-alpine@sha256:38a757619f8acbe316e20e926ee7c479b9300a50d3f462d6d3cf96a7ce551020 AS chef

WORKDIR /app

RUN cargo install cargo-chef

# Planner stage: Prepare recipe.json
FROM chef AS planner

COPY . .

RUN cargo chef prepare --recipe-path recipe.json

# Builder stage: Build dependencies and application
FROM chef AS builder

# Install required dependencies for musl build
RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static

COPY --from=planner /app/recipe.json recipe.json

# Build dependencies (this layer will be cached)
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

# Copy source code and build application
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --package painless-ghicon-web && \
    cp /app/target/release/painless-ghicon-web /tmp/painless-ghicon-web

# Runtime stage: Create minimal production image with static binary
FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab

WORKDIR /app

COPY --from=builder /tmp/painless-ghicon-web .

EXPOSE 8080

CMD ["./painless-ghicon-web"]
