"""The client version tracks the binary it drives.

Two version numbers for one thing is a question nobody should have to
answer -- "which client works with which pixelactions?" has no good
answer except "the same number". This fails if they drift.
"""

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[3]


def _read(path: pathlib.Path, pattern: str) -> str:
    match = re.search(pattern, path.read_text(), re.M)
    assert match, f"no {pattern!r} in {path}"
    return match.group(1)


def test_client_version_matches_the_workspace():
    client = _read(
        pathlib.Path(__file__).resolve().parents[1] / "pyproject.toml",
        r'^version = "([0-9]+\.[0-9]+\.[0-9]+)"',
    )
    workspace = _read(ROOT / "Cargo.toml", r'^version = "([0-9]+\.[0-9]+\.[0-9]+)"')
    assert client == workspace, (
        f"the Python client says {client} and the workspace says {workspace}. "
        "They are one product; move them together."
    )
