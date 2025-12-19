FROM alpine:3.21 AS builder

# Install build dependencies
RUN apk add --no-cache \
    rust \
    cargo \
    musl-dev \
    pkgconfig \
    php84-dev \
    php84-embed \
    clang \
    llvm \
    make \
    autoconf \
    g++ \
    libffi-dev \
    libxml2-dev \
    argon2-dev \
    openssl-dev \
    curl-dev \
    oniguruma-dev \
    sqlite-dev \
    zlib-dev

# Set up PHP paths
ENV PHP_CONFIG=/usr/bin/php-config84
ENV PKG_CONFIG_PATH=/usr/lib/pkgconfig

# Create symlink for libphp (library is in /usr/lib/php84/)
RUN ln -sf /usr/lib/php84/libphp.so /usr/lib/libphp.so

# Create working directory
WORKDIR /app

# Copy source files
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY build.rs ./

# Build the application
RUN RUSTFLAGS="-C target-feature=-crt-static" cargo build --release

# Runtime stage
FROM alpine:3.21

# Install runtime dependencies
RUN apk add --no-cache \
    php84-embed \
    php84-common \
    php84-session \
    php84-mbstring \
    php84-openssl \
    php84-curl \
    php84-pdo \
    php84-pdo_mysql \
    php84-pdo_pgsql \
    php84-pdo_sqlite \
    libgcc \
    && ln -sf /usr/lib/php84/libphp.so /usr/lib/libphp.so \
    && mkdir -p /tmp

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
