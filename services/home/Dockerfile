FROM ghcr.io/astral-sh/uv:0.8.15-python3.13-bookworm-slim

WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --locked --no-dev
COPY src ./src

ENV CELERITY_HOME_STORAGE_ROOT=/var/lib/celerity-home
ENV CELERITY_HOME_TOKEN_PATH=/var/run/secrets/celerity/token
USER 65532:65532
EXPOSE 8080
CMD ["uv", "run", "--no-sync", "python", "-m", "celerity_home"]
