FROM php:8.4-zts-alpine AS builder

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

# Copy source files
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY build.rs ./

# Build the application
RUN RUSTFLAGS="-C target-feature=-crt-static" cargo build --release

# Runtime stage - use same ZTS image
FROM php:8.4-zts-alpine

# Install runtime dependencies
RUN apk add --no-cache libgcc

# Create app directory
WORKDIR /app

# Copy the built binary
COPY --from=builder /app/target/release/tokio_php /usr/local/bin/tokio_php

# Create directory for PHP files
RUN mkdir -p /var/www/html

# Copy PHP files
COPY www /var/www/html

EXPOSE 8080

CMD ["tokio_php"]
