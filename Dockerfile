FROM --platform=linux/amd64 rust:1.90.0 AS builder

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
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-unknown-linux-gnu

WORKDIR /app
COPY . .

RUN cargo build --release --target x86_64-unknown-linux-gnu

# Extract stage - copy binaries to a clean layer
FROM scratch AS export
COPY --from=builder /app/target/x86_64-unknown-linux-gnu/release/ /
