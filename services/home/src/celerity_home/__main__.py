"""Uvicorn entry point for the one-process home application."""

from __future__ import annotations

import os
from pathlib import Path

import uvicorn

from celerity_home.application import create_app


def main() -> None:
    storage_root = Path(os.environ["CELERITY_HOME_STORAGE_ROOT"])
    token_path = Path(os.environ["CELERITY_HOME_TOKEN_PATH"])
    webhook_url = os.environ["CELERITY_HOME_WEBHOOK_URL"]
    app = create_app(storage_root, token_path.read_text().strip(), webhook_url)
    uvicorn.run(app, host="0.0.0.0", port=8080, access_log=False)


if __name__ == "__main__":
    main()
