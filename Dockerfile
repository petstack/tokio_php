# PHP version: 8.4 or 8.5
ARG PHP_VERSION=8.4
# Cargo features: debug-profile, grpc, otel (default: grpc,otel)
ARG CARGO_FEATURES="grpc,otel"

FROM php:${PHP_VERSION}-zts-alpine AS builder

# Install Rust and build dependencies (including PHP library dependencies)
RUN apk add --no-cache \
    rust \
    cargo \
    musl-dev \
    pkgconfig \
    clang \
    llvm \
    make \
    g++ \
    autoconf \
    automake \
    # protoc for gRPC proto compilation
    protobuf \
    protobuf-dev \
    # Libraries required by PHP
    readline-dev \
    ncurses-dev \
    curl-dev \
    oniguruma-dev \
    sqlite-dev \
    argon2-dev \
    libxml2-dev \
    zlib-dev \
    openssl-dev \
    gnu-libiconv-dev

# PHP paths are already set in this image:
# - libphp.so at /usr/local/lib/libphp.so
# - headers at /usr/local/include/php/
# - php-config at /usr/local/bin/php-config

# Create working directory
WORKDIR /app

# Copy extension source files
COPY ext ./ext

# Build tokio_bridge shared library first (shared TLS for Rust <-> PHP)
WORKDIR /app/ext/bridge
RUN make && \
    make install && \
    ls -la /usr/local/lib/libtokio_bridge.so && \
    cp /usr/local/lib/libtokio_bridge.so /usr/lib/ && \
    cp /usr/local/lib/libtokio_bridge.so /lib/ && \
    # Set up musl library path for both x86_64 and aarch64
    ldconfig 2>/dev/null || { \
        ARCH=$(uname -m); \
        echo "/usr/local/lib" >> /etc/ld-musl-${ARCH}.path 2>/dev/null || \
        echo "/usr/local/lib" >> /etc/ld-musl-aarch64.path 2>/dev/null || \
        echo "/usr/local/lib" >> /etc/ld-musl-x86_64.path 2>/dev/null || true; \
    }

# Re-declare ARG CARGO_FEATURES for use in builder stage (Docker requirement)
ARG CARGO_FEATURES

# Build tokio_sapi PHP extension
# When tokio-sapi feature is enabled: build static library only (for FFI functions)
# Otherwise: build both shared (.so) and static (.a) libraries
WORKDIR /app/ext

# Always run phpize and configure (generates config.h needed for compilation)
RUN phpize && ./configure --enable-tokio_sapi

# Build shared library only if tokio-sapi feature is NOT enabled
RUN if echo "$CARGO_FEATURES" | grep -q "tokio-sapi"; then \
        echo "=== Skipping shared extension build (tokio-sapi enabled) ==="; \
        touch /tmp/skip_c_ext; \
        mkdir -p modules && touch modules/.placeholder; \
    else \
        make EXTRA_LDFLAGS="/usr/local/lib/libtokio_bridge.so" && \
        make install; \
    fi

# Save extension directory path for runtime stage
RUN php-config --extension-dir > /tmp/php_ext_dir

# Always build static library for linking with Rust (needed for superglobals functions)
# Note: Even with tokio-sapi feature, we need FFI functions for $_GET, $_POST, $_FILES
RUN cc -c -fPIC -I. -I./bridge -I/usr/local/include/php -I/usr/local/include/php/main \
    -I/usr/local/include/php/TSRM -I/usr/local/include/php/Zend \
    -I/usr/local/include/php/ext -DHAVE_CONFIG_H -o tokio_sapi_static.o tokio_sapi.c && \
    ar rcs libtokio_sapi.a tokio_sapi_static.o && \
    cp libtokio_sapi.a /usr/local/lib/

# Back to app directory
WORKDIR /app

# Copy source files
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY build.rs ./
# Proto files for gRPC (optional, only used with --features grpc)
COPY proto ./proto

# Set library paths for linking with tokio_bridge
ENV LIBRARY_PATH=/usr/local/lib:/usr/lib:/lib
ENV LD_LIBRARY_PATH=/usr/local/lib:/usr/lib:/lib
ENV RUSTFLAGS="-C target-feature=-crt-static -L/usr/local/lib -L/usr/lib -L/lib"

# Verify tokio_bridge library exists in expected locations
RUN echo "=== Library locations ===" && \
    ls -la /usr/local/lib/libtokio_bridge.so && \
    ls -la /usr/lib/libtokio_bridge.so && \
    ls -la /lib/libtokio_bridge.so && \
    echo "=== ld-musl paths ===" && \
    cat /etc/ld-musl-*.path 2>/dev/null || echo "No ld-musl path files"

# Re-declare ARG after FROM (Docker requirement)
ARG CARGO_FEATURES

# Run unit tests before building (fail fast if tests don't pass)
# Note: --lib runs library unit tests, --bin tokio_php runs binary tests
# Both exclude integration tests which require running server
RUN if [ -n "$CARGO_FEATURES" ]; then \
        echo "=== Running tests with features: $CARGO_FEATURES ===" && \
        cargo test --release --lib --features "$CARGO_FEATURES" && \
        cargo test --release --bin tokio_php --features "$CARGO_FEATURES"; \
    else \
        cargo test --release --lib && cargo test --release --bin tokio_php; \
    fi

# Build the application
RUN if [ -n "$CARGO_FEATURES" ]; then \
        echo "=== Building with features: $CARGO_FEATURES ===" && \
        cargo build --release --features "$CARGO_FEATURES"; \
    else \
        cargo build --release; \
    fi

# Runtime stage - use same ZTS image
ARG PHP_VERSION=8.4
FROM php:${PHP_VERSION}-zts-alpine

# Install runtime dependencies and create www-data user
RUN apk add --no-cache libgcc curl && \
    addgroup -g 82 -S www-data 2>/dev/null || true && \
    adduser -u 82 -D -S -G www-data www-data 2>/dev/null || true

# Copy extension directory path from builder
COPY --from=builder /tmp/php_ext_dir /tmp/php_ext_dir

# Copy tokio_bridge shared library (required by both tokio_php binary and PHP extension)
COPY --from=builder /usr/local/lib/libtokio_bridge.so /usr/local/lib/libtokio_bridge.so
RUN ldconfig 2>/dev/null || echo "/usr/local/lib" >> /etc/ld-musl-x86_64.path

# Copy tokio_sapi extension from builder to correct PHP extensions directory
# Skip if tokio-sapi feature is enabled (uses pure Rust SAPI instead)
# Uses dynamic path detection to support PHP 8.4 and 8.5
RUN EXT_DIR=$(php-config --extension-dir) && \
    mkdir -p "$EXT_DIR"
# Copy skip_c_ext flag and optionally tokio_sapi.so (may be .placeholder if tokio-sapi enabled)
COPY --from=builder /tmp/skip_c_ext* /tmp/php_ext_dir /tmp/
COPY --from=builder /app/ext/modules/ /tmp/ext_modules/
RUN if [ -f /tmp/skip_c_ext ]; then \
        echo "=== Skipping C extension install (tokio-sapi enabled) ==="; \
        rm -rf /tmp/skip_c_ext /tmp/php_ext_dir /tmp/ext_modules; \
    else \
        EXT_DIR=$(php-config --extension-dir) && \
        cp /tmp/ext_modules/tokio_sapi.so "$EXT_DIR/" && \
        rm -rf /tmp/ext_modules /tmp/php_ext_dir && \
        echo "extension=tokio_sapi.so" >> /usr/local/etc/php/conf.d/tokio_sapi.ini; \
    fi

# Create app directory
WORKDIR /app

# Copy the built binary
COPY --from=builder /app/target/release/tokio_php /usr/local/bin/tokio_php

# Create directory for PHP files with proper ownership
RUN mkdir -p /var/www/html && chown -R www-data:www-data /var/www/html

# Copy PHP files
COPY --chown=www-data:www-data www/ /var/www/html/

EXPOSE 8080

# Run as non-root user
USER www-data

CMD ["tokio_php"]
