"""The wire, and nothing else.

One JSON object per line in, one back. Framing and vocabulary live here so
`Session` can be about ergonomics; the split also means the protocol can
be exercised in tests against a pipe with no binary present.

Mirrors `docs/PROTOCOL.md`. Where the two disagree, that document wins.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass, field
from typing import Any, Optional

#: The protocol revision this client speaks. The server answers `hello`
#: with the version it speaks; a mismatch is refused rather than guessed
#: at, because a client that gambles on an unknown protocol produces the
#: worst failure this tool has: input posted somewhere unintended.
PROTOCOL_VERSION = 1


class PixelactionsError(Exception):
    """Base for everything this package raises."""


class NotInstalled(PixelactionsError):
    """The `pixelactions` binary is not on PATH."""


class ProtocolError(PixelactionsError):
    """The server said something this client cannot act on.

    Covers a version mismatch, a malformed line, and the server's own
    `result: "error"` — a question it could not understand, such as an
    unknown label or an unreadable session.
    """


class StepFailed(PixelactionsError):
    """A step ran and honestly did not achieve what it was asked to.

    Distinct from `ProtocolError` on purpose, and it is the same line the
    exit codes draw: a verification that fails or a wait that times out is
    an *answer*, not a broken request. Catch this to handle a UI that did
    not do what you expected; catch `ProtocolError` to handle a flow that
    is wrong.
    """

    def __init__(self, verb: str, detail: str, report: "Step") -> None:
        super().__init__(f"{verb}: {detail}" if detail else verb)
        self.verb = verb
        self.detail = detail
        self.report = report


@dataclass(frozen=True)
class Point:
    """Where a step acted, after conversion."""

    x: float
    y: float
    space: str
    monitor: int
    scale: float


@dataclass(frozen=True)
class Step:
    """What one step did.

    `outcome` is the server's vocabulary: `verified` (it ran and the screen
    proves it), `executed` (it ran; nothing was checked afterwards), or
    `failed`. The distinction matters — the OS accepting an event says
    nothing about the application reacting to one.
    """

    outcome: str
    points: tuple[Point, ...] = ()
    detail: Optional[str] = None
    elapsed_ms: int = 0
    raw: dict[str, Any] = field(default_factory=dict, repr=False)

    @property
    def verified(self) -> bool:
        return self.outcome == "verified"

    @classmethod
    def from_response(cls, body: dict[str, Any]) -> "Step":
        return cls(
            outcome=body.get("outcome", "failed"),
            points=tuple(
                Point(
                    x=p["x"],
                    y=p["y"],
                    space=p.get("space", ""),
                    monitor=p.get("monitor", 0),
                    scale=p.get("scale", 1.0),
                )
                for p in body.get("points", ())
            ),
            detail=body.get("detail"),
            elapsed_ms=body.get("elapsed_ms", 0),
            raw=body,
        )


@dataclass(frozen=True)
class Located:
    """Where the regions are now, without acting.

    `missing` is the one to check: a label that cannot be found
    unambiguously is one that must not be acted on.
    """

    moved: tuple[str, ...]
    missing: tuple[str, ...]


class Wire:
    """Newline-delimited JSON over a child process's stdin and stdout."""

    def __init__(self, process: subprocess.Popen) -> None:
        self._process = process
        self._next_id = 0

    def send(self, verb: str, **fields: Any) -> dict[str, Any]:
        """One request, one response, ids checked.

        The id is verified rather than assumed. Only one request is in
        flight today, but the protocol carries an id so concurrency can
        arrive later without a breaking change — and a client that ignored
        it would silently mis-attribute answers on the day it does.
        """
        self._next_id += 1
        request = {"id": self._next_id, "do": verb}
        request.update({k: v for k, v in fields.items() if v is not None})

        stdin = self._process.stdin
        stdout = self._process.stdout
        if stdin is None or stdout is None:  # pragma: no cover - defensive
            raise ProtocolError("the server's pipes are closed")

        stdin.write(json.dumps(request) + "\n")
        stdin.flush()

        line = stdout.readline()
        if not line:
            raise ProtocolError(
                f"the server closed the connection during {verb!r}. "
                "Its stderr carries the reason; stdout is protocol only."
            )
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise ProtocolError(f"not a protocol line: {line!r}") from error

        if response.get("id") not in (None, self._next_id):
            raise ProtocolError(
                f"answer to request {response.get('id')} arrived for "
                f"request {self._next_id}"
            )
        if response.get("result") == "error":
            raise ProtocolError(response.get("detail", "no reason given"))
        return response
