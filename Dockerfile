FROM --platform=linux/amd64 rust:1.91.0 AS builder
ARG PROJECT_NAME=screencast
RUN apt-get update && \
    apt-get install -y \
    build-essential \
    gcc \
    g++ \
    llvm \
    clang \
    libclang-dev \
    lld \
    gcc-x86-64-linux-gnu \
    pkg-config \
    libpipewire-0.3-dev \
    libegl1-mesa-dev \
    libgl1-mesa-dev \
    libgles2-mesa-dev \
    libdrm-dev \
    libgbm-dev \
    libwayland-dev \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-unknown-linux-gnu

WORKDIR /${PROJECT_NAME}

COPY ./Cargo.toml .
COPY ./Cargo.lock .
COPY ./signal/ ./signal
COPY ./client/ ./client
COPY ./webrtc_model/ ./webrtc_model

RUN cargo build --release --target x86_64-unknown-linux-gnu

# Extract stage - copy binaries to a clean layer
FROM scratch AS export
ARG PROJECT_NAME=screencast
COPY --from=builder /${PROJECT_NAME}/target/x86_64-unknown-linux-gnu/release/signal /
COPY --from=builder /${PROJECT_NAME}/target/x86_64-unknown-linux-gnu/release/client /
