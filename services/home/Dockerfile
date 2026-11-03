FROM rust:1.99.1-bookworm AS home-build

ENV RUSTUP_TOOLCHAIN=1.99.0
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY crates ./crates
COPY firmware ./firmware
COPY gen/rust ./gen/rust
COPY tools/generate ./tools/generate
COPY sql ./sql
RUN cargo build --locked --release -p home-service --bin celerity-home

FROM ghcr.io/astral-sh/uv:0.9.30-python3.14-bookworm-slim

WORKDIR /app/services/home
ENV UV_LINK_MODE=copy \
    UV_PYTHON_DOWNLOADS=never \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1
COPY gen/python /app/gen/python
COPY services/home/pyproject.toml services/home/uv.lock ./
COPY services/home/src ./src
RUN uv sync --locked --no-dev --no-editable --no-cache
COPY --from=home-build /src/target/release/celerity-home /usr/local/bin/celerity-home

ENV CELERITY_HOME_STORAGE_ROOT=/var/lib/celerity-home \
    CELERITY_HOME_PYTHON=/app/services/home/.venv/bin/python
USER 65532:65532
EXPOSE 8080
CMD ["celerity-home"]
