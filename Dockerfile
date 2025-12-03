# Build stage
FROM rust:1.83.0-alpine AS build

# Install build dependencies (including static OpenSSL libs for musl)
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

# Set OpenSSL to static linking
ENV OPENSSL_STATIC=1

WORKDIR /build

# Copy source files
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM alpine:3.20 AS final

# Install runtime dependencies
RUN apk add --no-cache ca-certificates libgcc

WORKDIR /app

# Copy the binary
COPY --from=build /build/target/release/Horizon-Maestro /app/maestro

# Expose port
EXPOSE 8000

# Environment variables with defaults
ENV MAESTRO_AUTO_BOOTSTRAP=true
ENV MAESTRO_ATLAS_IMAGE=ghcr.io/far-beyond-dev/horizon-atlas:main
ENV MAESTRO_HORIZON_IMAGE=ghcr.io/far-beyond-dev/horizon:main
ENV MAESTRO_HORIZON_BASE_PORT=8080
ENV MAESTRO_REGION_SIZE=1000.0

# Run the application
ENTRYPOINT ["/app/maestro"]
