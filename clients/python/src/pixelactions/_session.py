"""The object a script holds."""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from types import TracebackType
from typing import Any, Optional

from ._protocol import (
    PROTOCOL_VERSION,
    Located,
    NotInstalled,
    ProtocolError,
    Step,
    StepFailed,
    Wire,
)

#: Method name -> the verb on the wire. They differ in three places, and
#: only where Python forces it: `type` and `from` are keywords, and `wait`
#: reads better than `wait_for` at a call site. Everything else is the
#: same word the flow file and the CLI use, because one vocabulary across
#: four surfaces is the point.
_VERBS = {
    "click": "click",
    "double_click": "double_click",
    "verify": "verify",
    "wait": "wait_for",
    "gone": "wait_gone",
    "changed": "changed",
}


class Session:
    """A live `pixelactions serve` process, driven by label.

    ```python
    with Session("~/captures/login") as ui:
        ui.click("email")
        ui.write("me@example.com")
        ui.click("submit")
        ui.wait("dashboard")
    ```

    Nothing is injected until a method is called, and every method raises
    rather than returning a failure you might not check -- automation that
    continues after a step did not land is automation doing damage
    quietly. Pass `strict=False` to get the report back instead.
    """

    def __init__(
        self,
        session: str | os.PathLike[str],
        *,
        binary: str = "pixelactions",
        strict: bool = True,
        settings: Optional[dict[str, Any]] = None,
    ) -> None:
        self._path = Path(session).expanduser()
        self._binary = binary
        self._strict = strict
        self._settings = settings
        self._process: Optional[subprocess.Popen] = None
        self._wire: Optional[Wire] = None
        self.verbs: tuple[str, ...] = ()

    # -- lifecycle ------------------------------------------------------

    def open(self) -> "Session":
        """Start the server and agree a protocol version."""
        if shutil.which(self._binary) is None:
            raise NotInstalled(
                f"{self._binary!r} is not on PATH. Install it with "
                "`brew install pixelactions` or `cargo install pixelactions`, "
                "then run `pixelactions doctor` to check permissions."
            )

        self._process = subprocess.Popen(
            [self._binary, "serve", "--session", str(self._path)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            # stderr is left alone: the protocol says logs go there and a
            # client must never read them as failure. Capturing it into a
            # pipe nobody drains would deadlock a chatty run.
            text=True,
            bufsize=1,
        )
        self._wire = Wire(self._process)

        welcome = self._wire.send(
            "hello", version=PROTOCOL_VERSION, settings=self._settings
        )
        spoken = welcome.get("version")
        if spoken != PROTOCOL_VERSION:
            self.close()
            raise ProtocolError(
                f"this client speaks protocol {PROTOCOL_VERSION}, the server "
                f"speaks {spoken}. Upgrade whichever is older rather than "
                "guessing at the difference."
            )
        self.verbs = tuple(welcome.get("verbs", ()))
        return self

    def close(self) -> None:
        """Say goodbye, then make sure the process is gone."""
        if self._wire is not None and self._process is not None:
            if self._process.poll() is None:
                try:
                    self._wire.send("bye")
                except (ProtocolError, OSError, ValueError):
                    # Closing stdin is the documented graceful shutdown, so
                    # a server that has already gone is not an error here.
                    pass
        if self._process is not None:
            for pipe in (self._process.stdin, self._process.stdout):
                if pipe is not None:
                    try:
                        pipe.close()
                    except OSError:  # pragma: no cover - defensive
                        pass
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:  # pragma: no cover
                self._process.kill()
        self._process = None
        self._wire = None

    def __enter__(self) -> "Session":
        return self.open()

    def __exit__(self, *_: object) -> None:
        self.close()

    # -- steps ----------------------------------------------------------

    def _step(self, verb: str, **fields: Any) -> Step:
        if self._wire is None:
            raise ProtocolError("this session is not open")
        report = Step.from_response(self._wire.send(verb, **fields))
        if self._strict and report.outcome == "failed":
            raise StepFailed(verb, report.detail or "", report)
        return report

    def click(self, target: str) -> Step:
        return self._step("click", target=target)

    def double_click(self, target: str) -> Step:
        return self._step("double_click", target=target)

    def verify(self, target: str) -> Step:
        """Is the region still what it was?"""
        return self._step("verify", target=target)

    def wait(self, target: str) -> Step:
        """Block until the region matches again."""
        return self._step("wait_for", target=target)

    def gone(self, target: str) -> Step:
        """Block until the region stops matching."""
        return self._step("wait_gone", target=target)

    def changed(self, target: str, tolerance: Optional[float] = None) -> Step:
        """Did this region change? The strongest post-action check."""
        return self._step("changed", target=target, tolerance=tolerance)

    def write(self, text: str) -> Step:
        """Type text. Named for pyautogui's `write`, since `type` is a
        Python keyword and `type_` reads like an apology."""
        return self._step("type", text=text)

    def key(self, chord: str) -> Step:
        """A chord like `cmd+s`, arriving as keys rather than characters."""
        return self._step("key", chord=chord)

    def drag(self, start: str, end: str) -> Step:
        # `from` is a keyword in Python and a field name on the wire.
        return self._step("drag", **{"from": start, "to": end})

    def scroll(self, target: str, amount: int, *, horizontal: bool = False) -> Step:
        return self._step(
            "scroll",
            target=target,
            amount=amount,
            axis="horizontal" if horizontal else None,
        )

    def pause(self, ms: int) -> Step:
        return self._step("pause", ms=ms)

    def relocate(self) -> Located:
        """Re-locate every region and report, without acting.

        Check `missing` before a run that matters: a label that cannot be
        found unambiguously is one that must not be acted on.
        """
        if self._wire is None:
            raise ProtocolError("this session is not open")
        body = self._wire.send("relocate")
        return Located(
            moved=tuple(body.get("moved", ())),
            missing=tuple(body.get("missing", ())),
        )
