# Linux x86_64 static musl build
FROM --platform=linux/amd64 rust:alpine AS builder

RUN apk add --no-cache \
    musl-dev \
    pipewire-dev \
    pkgconf \
    clang-dev \
    llvm-static

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /usr/src/screencast

COPY Cargo.toml ./
COPY client ./client/
COPY signal ./signal/
COPY webrtc_model ./webrtc_model/

ENV LIBCLANG_STATIC=1
ENV LLVM_CONFIG_PATH=/usr/lib/llvm20/bin/llvm-config
RUN cargo build --jobs 8 --release --target x86_64-unknown-linux-musl --workspace
