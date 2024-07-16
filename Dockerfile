FROM rust:1.74 AS builder

WORKDIR /build

RUN apt-get update
RUN apt-get install -y git clang cmake libsnappy-dev

RUN git clone https://github.com/comit-network/xmr-btc-swap .

RUN cargo build --release --bin=asb

FROM debian:bullseye-slim

WORKDIR /data

COPY --from=builder /build/target/release/asb /bin/asb

ENTRYPOINT ["asb"]
