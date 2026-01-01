# PHP version: 8.4 or 8.5
ARG PHP_VERSION=8.4

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

# Build tokio_sapi PHP extension as shared library
COPY ext ./ext
WORKDIR /app/ext
RUN phpize && \
    ./configure --enable-tokio_sapi && \
    make && \
    make install

# Save extension directory path for runtime stage
RUN php-config --extension-dir > /tmp/php_ext_dir

# Also build as static library for linking with Rust
RUN cc -c -fPIC -I. -I/usr/local/include/php -I/usr/local/include/php/main \
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

# Run unit tests before building (fail fast if tests don't pass)
# Note: --bin tokio_php excludes integration tests which require running server
RUN cargo test --no-default-features --release --bin tokio_php

# Build the application
RUN RUSTFLAGS="-C target-feature=-crt-static" cargo build --release

# Runtime stage - use same ZTS image
ARG PHP_VERSION=8.4
FROM php:${PHP_VERSION}-zts-alpine

# Install runtime dependencies and create www-data user
RUN apk add --no-cache libgcc curl && \
    addgroup -g 82 -S www-data 2>/dev/null || true && \
    adduser -u 82 -D -S -G www-data www-data 2>/dev/null || true

# Copy extension directory path from builder
COPY --from=builder /tmp/php_ext_dir /tmp/php_ext_dir

# Copy tokio_sapi extension from builder to correct PHP extensions directory
# Uses dynamic path detection to support PHP 8.4 and 8.5
RUN EXT_DIR=$(php-config --extension-dir) && \
    mkdir -p "$EXT_DIR"
COPY --from=builder /app/ext/modules/tokio_sapi.so /tmp/tokio_sapi.so
RUN EXT_DIR=$(php-config --extension-dir) && \
    cp /tmp/tokio_sapi.so "$EXT_DIR/" && \
    rm /tmp/tokio_sapi.so /tmp/php_ext_dir && \
    echo "extension=tokio_sapi.so" >> /usr/local/etc/php/conf.d/tokio_sapi.ini

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
