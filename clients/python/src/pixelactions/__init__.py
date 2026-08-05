"""Drive desktop interactions from a session a human marked.

    from pixelactions import Session

    with Session("~/captures/login") as ui:
        ui.click("email")
        ui.write("me@example.com")
        ui.key("tab")
        ui.click("submit")
        ui.wait("dashboard")

Every action names a *label* a person marked in pixelcoords, never a
coordinate. The binary relocates the region before acting and can verify
after, so a script survives the window moving -- which is the failure that
makes coordinate-based automation rot.

This package speaks the line protocol (`docs/PROTOCOL.md`) to a
`pixelactions serve` child process. No FFI, no native module, no
dependencies.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from types import TracebackType
from typing import Any, Optional

from ._protocol import (
    PROTOCOL_VERSION,
    Located,
    NotInstalled,
    PixelactionsError,
    Point,
    ProtocolError,
    Step,
    StepFailed,
    Wire,
)

__all__ = [
    "Session",
    "Step",
    "Point",
    "Located",
    "PixelactionsError",
    "NotInstalled",
    "ProtocolError",
    "StepFailed",
    "PROTOCOL_VERSION",
]

__version__ = "0.9.7"


class Session:
    """A live `pixelactions serve` process, pointed at one session.

    Use it as a context manager so the child is always closed:

        with Session("~/captures/login") as ui:
            ui.click("submit")

    Steps raise :class:`StepFailed` when they run and do not achieve what
    they were asked -- a verification that fails, a wait that times out.
    That is deliberate: in a script, a silent false return is how a
    failing automation keeps going and does damage. Pass
    ``raise_on_failure=False`` to get the report back instead.
    """

    def __init__(
        self,
        session: str,
        *,
        binary: str = "pixelactions",
        settings: Optional[dict[str, Any]] = None,
        raise_on_failure: bool = True,
    ) -> None:
        self._path = os.path.expanduser(str(session))
        self._binary = binary
        self._settings = settings
        self._raise = raise_on_failure
        self._process: Optional[subprocess.Popen] = None
        self._wire: Optional[Wire] = None
        #: Verbs the server said it understands, from the handshake. A
        #: client is told to read this and degrade gracefully rather than
        #: guess -- see `verbs`.
        self.verbs: tuple[str, ...] = ()

    # -- lifecycle ---------------------------------------------------

    def open(self) -> "Session":
        if self._process is not None:
            return self
        if shutil.which(self._binary) is None:
            raise NotInstalled(
                f"{self._binary!r} is not on PATH. Install it with "
                "`brew install pixelactions` or `cargo install pixelactions`, "
                "then check the pairing with `pixelactions doctor`."
            )

        self._process = subprocess.Popen(
            [self._binary, "serve", "--session", self._path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            # stderr is left alone on purpose: the protocol says stdout is
            # protocol only and stderr is logs, so capturing it here would
            # silence the server's diagnostics with nobody reading them.
            text=True,
            bufsize=1,
        )
        self._wire = Wire(self._process)

        welcome = self._wire.send("hello", version=PROTOCOL_VERSION, settings=self._settings)
        served = welcome.get("version")
        if served != PROTOCOL_VERSION:
            self.close()
            raise ProtocolError(
                f"this client speaks protocol {PROTOCOL_VERSION}, the server "
                f"speaks {served}. Upgrade whichever is older rather than "
                "guessing -- a mismatched guess posts real input."
            )
        self.verbs = tuple(welcome.get("verbs", ()))
        return self

    def close(self) -> None:
        """End the session. Closing stdin does the same thing."""
        if self._process is None:
            return
        try:
            if self._wire is not None and self._process.poll() is None:
                try:
                    self._wire.send("bye")
                except PixelactionsError:
                    pass  # already gone; the kill below settles it
        finally:
            for pipe in (self._process.stdin, self._process.stdout):
                if pipe is not None:
                    try:
                        pipe.close()
                    except OSError:
                        pass
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:  # pragma: no cover - defensive
                self._process.kill()
            self._process = None
            self._wire = None

    def __enter__(self) -> "Session":
        return self.open()

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- the verbs ---------------------------------------------------

    def click(self, target: str) -> Step:
        """Click the resolved point of a labeled region."""
        return self._step("click", target=target)

    def double_click(self, target: str) -> Step:
        return self._step("double_click", target=target)

    def write(self, text: str) -> Step:
        """Type text. Named `write` because `type` is a Python keyword."""
        return self._step("type", text=text)

    def key(self, chord: str) -> Step:
        """Send a chord like ``cmd+s`` -- as keys, not as characters."""
        return self._step("key", chord=chord)

    def drag(self, start: str, end: str) -> Step:
        """Press at one region, release at another."""
        return self._step("drag", **{"from": start, "to": end})

    def scroll(self, target: str, amount: int, *, horizontal: bool = False) -> Step:
        """Wheel over a region. Negative amounts go the other way.

        Always reports `executed`, never `verified`: a scroll changes its
        own evidence, so ask separately with :meth:`verify` or :meth:`wait`.
        """
        axis = "horizontal" if horizontal else None
        return self._step("scroll", target=target, amount=amount, axis=axis)

    def verify(self, target: str) -> Step:
        """Is the region still what it was?"""
        return self._step("verify", target=target)

    def wait(self, target: str) -> Step:
        """Block until the region matches its saved crop again."""
        return self._step("wait_for", target=target)

    def gone(self, target: str) -> Step:
        """Block until the region stops matching -- a spinner disappearing."""
        return self._step("wait_gone", target=target)

    def changed(self, target: str, *, tolerance: Optional[float] = None) -> Step:
        """Did this region change? The strongest post-action check there is."""
        return self._step("changed", target=target, tolerance=tolerance)

    def pause(self, ms: int) -> Step:
        """Wait a fixed time, for when there is genuinely no observable."""
        return self._step("pause", ms=ms)

    def relocate(self) -> Located:
        """Find every region now, without acting.

        `missing` is the one to check: a label that cannot be found
        unambiguously must not be acted on.
        """
        body = self._send("relocate")
        return Located(
            moved=tuple(body.get("moved", ())),
            missing=tuple(body.get("missing", ())),
        )

    # -- plumbing ----------------------------------------------------

    def _send(self, verb: str, **fields: Any) -> dict[str, Any]:
        if self._wire is None:
            raise PixelactionsError(
                "this session is not open -- use it as a context manager, "
                "or call open() first"
            )
        return self._wire.send(verb, **fields)

    def _step(self, verb: str, **fields: Any) -> Step:
        report = Step.from_response(self._send(verb, **fields))
        if self._raise and report.outcome == "failed":
            raise StepFailed(verb, report.detail or "", report)
        return report
