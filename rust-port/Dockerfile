FROM rust:slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY templates/ templates/
RUN cargo build --release --bin openfuse && \
    strip target/release/openfuse

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/openfuse /usr/local/bin/openfuse
EXPOSE 2053
ENTRYPOINT ["openfuse"]
CMD ["serve", "--store", "/store", "--port", "2053"]
