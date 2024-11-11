# This Dockerfile builds the asb binary

FROM rust:1.79-slim AS builder

WORKDIR /build

RUN apt-get update
RUN apt-get install -y git clang cmake libsnappy-dev

COPY . .

WORKDIR /build/swap

RUN cargo build --release --bin=asb

FROM debian:bookworm-slim

WORKDIR /data

COPY --from=builder /build/target/release/asb /bin/asb

ENTRYPOINT ["asb"]
