##install buildx beforehand

#set up buildx and test
#docker buildx create --use --platform=linux/arm64,linux/amd64 --name multi-platform-builder
#docker buildx inspect --bootstrap

#build command
#docker buildx build --platform linux/amd64,linux/arm64 -t rustical:latest .


# Base image for cross-compiling with cargo-chef
FROM --platform=$BUILDPLATFORM rust:1.84-alpine AS chef

ARG TARGETPLATFORM
ARG BUILDPLATFORM

# Set cross-compilation environment variables for different architectures
RUN case "$TARGETPLATFORM" in \
      "linux/amd64") echo "x86_64-unknown-linux-musl" > /tmp/rust_target ;; \
      #"linux/arm64") echo "aarch64-unknown-linux-musl" > /tmp/rust_target ;; \
      *) echo "Unsupported platform $TARGETPLATFORM" && exit 1 ;; \
    esac

RUN apk add --no-cache musl-dev llvm19 clang \
    && rustup target add "$(cat /tmp/rust_target)" \
    && cargo install cargo-chef --locked \
    && rm -rf "$CARGO_HOME/registry"

WORKDIR /app

# Planner stage
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Builder stage
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Install dependencies for cross-compilation (platform-specific)
RUN case ${TARGETPLATFORM} in \
        #"linux/arm64") apk add --no-cache protobuf-compiler g++-aarch64-linux-gnu libc6-dev-arm64-cross libssl-dev:arm64 ca-certificates ;; \
        "linux/amd64") apk add --no-cache protobuf-compiler g++-x86_64-linux-gnu libc6-dev-x86_64-cross libssl-dev ca-certificates ;; \
        *) exit 1 ;; \
    esac

# Build the project for the target platform
RUN case ${TARGETPLATFORM} in \
       # "linux/arm64") CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc cargo chef cook --release --target aarch64-unknown-linux-musl --recipe-path recipe.json ;; \
        "linux/amd64") cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json ;; \
        *) exit 1 ;; \
    esac

# Copy source code to be built
COPY . /app

# Final build step for target platform
RUN case ${TARGETPLATFORM} in \
        #"linux/arm64") CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-musl-gcc cargo build --release --target aarch64-unknown-linux-musl ;; \
        "linux/amd64") cargo build --release --target x86_64-unknown-linux-musl ;; \
        *) exit 1 ;; \
    esac

# Copy the compiled binary from the target directory
RUN set -ex; \
    case ${TARGETPLATFORM} in \
       # "linux/arm64") target='/app/target/aarch64-unknown-linux-musl/release';; \
        "linux/amd64") target='/app/target/x86_64-unknown-linux-musl/release';; \
        *) exit 1 ;; \
    esac; \
    cp $target/* /all-files/${TARGETPLATFORM}/app

# Runtime stage (minimal image)
FROM scratch AS runtime

ARG TARGETPLATFORM

WORKDIR /app

# Copy the necessary runtime files (binary and libs) for the target platform
COPY --from=builder /all-files/${TARGETPLATFORM} /

# Expose application port
EXPOSE 4000

# Entry point for the binary
ENTRYPOINT ["/app/rustical"]