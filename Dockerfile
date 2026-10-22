# Knut 2. Needs the vendor/mruby submodule checked out:
#   git submodule update --init

FROM rust:1-bookworm AS build
# mruby builds with rake.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ruby rake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
# tzdata: clock triggers and sun times use the local time zone (set TZ).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --home-dir /data knut \
    && mkdir /data /config \
    && chown knut:knut /data
COPY --from=build /src/target/release/knut /usr/local/bin/knut

USER knut
ENV KNUT_CONFIG=/config/config.rb \
    KNUT_STATE_DIR=/data
VOLUME /data
ENTRYPOINT ["knut"]
CMD ["run"]
