FROM rust:1.83-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p kaos-driver

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/kaos-driver /usr/local/bin/
ENTRYPOINT ["kaos-driver"]
CMD ["0.0.0.0:9000", "peer:9000"]
