"""What the client does with each shape of answer."""

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))

import pixelactions  # noqa: E402
from pixelactions import ProtocolError, Session, StepFailed  # noqa: E402

FAKE = pathlib.Path(__file__).resolve().parent / "fake_server.py"


def open_fake(mode: str = "ok", **kw):
    import subprocess
    from pixelactions._protocol import Wire

    s = Session("/fake", binary=sys.executable, **kw)
    s._process = subprocess.Popen(  # noqa: SLF001
        [sys.executable, str(FAKE), mode],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1,
    )
    s._wire = Wire(s._process)  # noqa: SLF001
    welcome = s._wire.send("hello", version=1)  # noqa: SLF001
    if welcome.get("version") != 1:
        s.close()
        raise ProtocolError("version mismatch")
    s.verbs = tuple(welcome.get("verbs", ()))
    return s


def test_a_step_reports_where_it_acted():
    ui = open_fake()
    try:
        step = ui.click("submit")
        assert step.outcome == "verified"
        assert step.verified
        assert step.points[0].x == 812.0
        assert step.points[0].space == "logical"
    finally:
        ui.close()


def test_the_handshake_advertises_the_verbs():
    ui = open_fake()
    try:
        # A client is told to read this rather than guess. Every verb the
        # client exposes must be one the server offered.
        for verb in ("click", "type", "changed", "scroll"):
            assert verb in ui.verbs
    finally:
        ui.close()


def test_a_failed_step_raises_with_the_reason():
    ui = open_fake("failed")
    try:
        try:
            ui.wait("dashboard")
        except StepFailed as error:
            assert "timed out" in str(error)
            assert error.report.outcome == "failed"
        else:
            raise AssertionError("a failed step should raise")
    finally:
        ui.close()


def test_a_failed_step_can_be_returned_instead():
    ui = open_fake("failed", raise_on_failure=False)
    try:
        step = ui.wait("dashboard")
        assert step.outcome == "failed"
        assert not step.verified
    finally:
        ui.close()


def test_a_malformed_question_is_a_different_exception():
    # The line the exit codes draw: a step that fails honestly is not the
    # same as a question the server could not understand.
    ui = open_fake("error")
    try:
        try:
            ui.click("nope")
        except ProtocolError as error:
            assert "nope" in str(error)
        else:
            raise AssertionError("an unknown label should raise ProtocolError")
    finally:
        ui.close()


def test_an_answer_to_the_wrong_request_is_refused():
    # Only one request is in flight today, but the id exists so
    # concurrency can arrive later. A client that ignored it would
    # mis-attribute answers on the day it does.
    ui = open_fake("wrong-id")
    try:
        try:
            ui.click("submit")
        except ProtocolError as error:
            assert "999" in str(error)
        else:
            raise AssertionError("a mismatched id should raise")
    finally:
        ui.close()


def test_relocate_reports_what_cannot_be_found():
    ui = open_fake()
    try:
        located = ui.relocate()
        assert located.moved == ("submit",)
        assert located.missing == ("gone-label",)
    finally:
        ui.close()


def test_a_missing_binary_says_how_to_install_it():
    try:
        Session("/fake", binary="definitely-not-a-real-binary").open()
    except pixelactions.NotInstalled as error:
        assert "brew install" in str(error) or "cargo install" in str(error)
    else:
        raise AssertionError("a missing binary should raise NotInstalled")


if __name__ == "__main__":
    # Runnable without pytest, so `python3 tests/test_session.py` works on a
    # machine that has nothing installed -- which is most machines someone
    # tries this on for the first time.
    import traceback

    failures = 0
    for name, fn in sorted(globals().items()):
        if not name.startswith("test_") or not callable(fn):
            continue
        try:
            fn()
            print(f"  ok    {name}")
        except Exception:
            failures += 1
            print(f"  FAIL  {name}")
            traceback.print_exc()
    raise SystemExit(1 if failures else 0)
