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

# Build the application
RUN RUSTFLAGS="-C target-feature=-crt-static" cargo build --release

# Runtime stage - use same ZTS image
FROM php:8.4-zts-alpine

# Install runtime dependencies
RUN apk add --no-cache libgcc

# Copy tokio_sapi extension from builder to correct PHP extensions directory
COPY --from=builder /usr/local/lib/php/extensions/no-debug-zts-20240924/tokio_sapi.so \
     /usr/local/lib/php/extensions/no-debug-zts-20240924/

# Configure tokio_sapi extension
RUN echo "extension=tokio_sapi.so" >> /usr/local/etc/php/conf.d/tokio_sapi.ini

# Configure OPcache + JIT - works by overriding SAPI name to "cli-server" before init
RUN echo "opcache.enable=1" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.enable_cli=1" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.memory_consumption=128" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.interned_strings_buffer=16" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.max_accelerated_files=10000" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.validate_timestamps=0" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.revalidate_freq=0" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.jit_buffer_size=64M" >> /usr/local/etc/php/conf.d/opcache.ini && \
    echo "opcache.jit=tracing" >> /usr/local/etc/php/conf.d/opcache.ini

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
