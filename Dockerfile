# gopher-blog: an *immutable* phlog image. The renderer runs at BUILD time
# against the (public) debene-dev content and bakes the static gopher tree into
# the image; the final layer is geomyidae serving that baked tree. There is no
# runtime renderer and no loop — when the content changes you rebuild the image,
# and Watchtower swapping the container IS the atomic publish.
#
#   docker build -t gopher-blog .      # clones debene-dev@main at build time
#   docker run --rm -p 7071:7071 gopher-blog
#   lynx gopher://127.0.0.1:7071/

# --- 1. Build the renderer ------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release

# --- 2. Render the tree against the debene-dev content --------------------
FROM build AS render
ARG CONTENT_REPO=https://github.com/felipedbene/debene-dev
ARG CONTENT_REF=main
ARG GOPHER_HOST=gopher.debene.dev
ARG GOPHER_PORT=7071
ARG CTA_LINK=gopher://gopher.debene.dev:70
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch "$CONTENT_REF" "$CONTENT_REPO" /content
# Render, then dereference the `current` symlink so the final image carries a
# plain tree (no symlink indirection) at a fixed path.
RUN /src/target/release/gopher-blog \
      --content /content/content \
      --out /build/out \
      --host "$GOPHER_HOST" \
      --port "$GOPHER_PORT" \
      --cta-link "$CTA_LINK" \
 && cp -rL /build/out/current /export \
 && echo "baked $(find /export -type f | wc -l) files"

# --- 3. Build geomyidae (mirrors gopher-cta deploy/Dockerfile.geomyidae) --
# Not packaged in Debian; build from the canonical bitreich source over git://
# (the HTTPS "tarball" returns HTML). Build host needs port 9418 egress. TLS
# support disabled (no gophers), so the runtime needs only libc.
FROM debian:bookworm-slim AS geo
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates gcc make libc6-dev \
 && rm -rf /var/lib/apt/lists/*
ARG GEOMYIDAE_REF=v0.99
RUN git clone git://bitreich.org/geomyidae /g \
 && cd /g && git checkout "$GEOMYIDAE_REF" \
 && make TLS_CFLAGS= TLS_LDFLAGS=

# --- 4. Runtime: geomyidae serving the baked tree on :7071 ----------------
FROM debian:bookworm-slim
COPY --from=geo /g/geomyidae /usr/local/bin/geomyidae
COPY --from=render /export /srv
USER nobody:nogroup
EXPOSE 7071
# -d: foreground (container stays up). -b: serve the baked tree. -p: port.
# The .gph links already carry concrete host/port (baked by the renderer's
# --host/--port), so geomyidae's own -h substitution isn't needed here.
ENTRYPOINT ["geomyidae", "-d", "-b", "/srv", "-p", "7071"]
