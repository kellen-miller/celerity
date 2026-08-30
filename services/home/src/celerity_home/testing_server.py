"""Real HTTP entry point used by the cross-language acceptance exercise."""

from __future__ import annotations

import os
from pathlib import Path

from celerity_home.application import create_app

app = create_app(
    Path(os.environ["CELERITY_HOME_STORAGE_ROOT"]),
    os.environ["CELERITY_HOME_VEHICLE_TOKEN"],
    os.environ["CELERITY_HOME_WEBHOOK_URL"],
)
