# This Dockerfile builds the asb binary

FROM ubuntu:24.04 AS builder

WORKDIR /build

# Install dependencies
# See .github/workflows/action.yml as well
RUN apt-get update && \
    apt-get install -y \
        git \
        curl \
        clang \
        libsnappy-dev \
        build-essential \
        cmake \
        libboost-all-dev \
        miniupnpc \
        libunbound-dev \
        graphviz \
        doxygen \
        libunwind8-dev \
        pkg-config \
        libssl-dev \
        libzmq3-dev \
        libsodium-dev \
        libhidapi-dev \
        libabsl-dev \
        libusb-1.0-0-dev \
        libprotobuf-dev \
        protobuf-compiler \
        libnghttp2-dev \
        libevent-dev \
        libexpat1-dev \
        ccache && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install Rust 1.85
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain 1.85.0
ENV PATH="/root/.cargo/bin:${PATH}"

COPY . .

# Check that submodules are present (they should be initialized before building)
RUN if [ ! -f "monero-sys/monero/CMakeLists.txt" ]; then \
        echo "ERROR: Submodules not initialized. Run 'git submodule update --init --recursive' before building Docker image."; \
        exit 1; \
    fi

WORKDIR /build/swap

# Act as if we are in a GitHub Actions environment
ENV DOCKER_BUILD=true

RUN cargo build -vv --bin=asb

FROM ubuntu:24.04

WORKDIR /data

COPY --from=builder /build/target/debug/asb /bin/asb

ENTRYPOINT ["asb"]