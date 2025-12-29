FROM rust:1.78 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libasound2 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/airlift-node /app/airlift-node
EXPOSE 8087
ENV RUST_LOG=info
CMD ["/app/airlift-node"]
