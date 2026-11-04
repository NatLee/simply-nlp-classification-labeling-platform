# 分類標注平台：一個映像檔包含 Rust 後端和 React 前端。

# 1. 前端
FROM node:24-trixie-slim AS web
WORKDIR /web
RUN corepack enable && corepack prepare pnpm@10.6.5 --activate
COPY src/web/package.json src/web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY src/web/ ./
RUN pnpm build

# 2. 後端（examples/ 裡的範例檔會編進執行檔，匯入頁可以下載）
FROM rust:1-trixie AS api
WORKDIR /build
COPY examples examples
COPY src/api/Cargo.toml src/api/Cargo.lock src/api/rustfmt.toml src/api/
COPY src/api/migrations src/api/migrations
COPY src/api/src src/api/src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/src/api/target \
    cd src/api && cargo build --release --locked \
    && install -D target/release/simply-nlp-labeling /out/simply-nlp-labeling

# 3. 執行環境
FROM debian:trixie-slim
# ca-certificates：連 Claude API 或其他 HTTPS 的 AI 服務要用
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home --home-dir /home/app --shell /usr/sbin/nologin app \
    && mkdir -p /app/data \
    && chown app /app/data
WORKDIR /app
COPY --from=api /out/simply-nlp-labeling /usr/local/bin/simply-nlp-labeling
COPY --from=web /web/dist /app/web
ENV LABEL_BIND=0.0.0.0:5000 \
    LABEL_DATA_DIR=/app/data \
    LABEL_STATIC_DIR=/app/web
VOLUME /app/data
EXPOSE 5000
USER app
CMD ["simply-nlp-labeling"]
