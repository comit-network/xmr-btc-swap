FROM rust:1.59-slim AS builder

WORKDIR /build

RUN apt-get update
RUN apt-get install -y git clang cmake libsnappy-dev

COPY . .

RUN cargo build --release --bin=asb

FROM debian:bullseye-slim

WORKDIR /data

COPY --from=builder /build/target/release/asb /bin/asb

ENTRYPOINT ["asb"]
