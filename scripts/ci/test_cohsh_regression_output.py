# Author: Lukas Bower
# Purpose: Preserve exact append ordering in streamed audit records.
# Copyright 2026 Lukas Bower
"""Independent stream examples for the canonical three-append fixture."""

import pytest

from cohsh_regression_output import verify_append_stream


def stream(records: list[str], preview: str = "boot") -> str:
    """Frame one client-rendered CAT response, including unrelated audit rows."""
    return (f"[console] OK CAT path=/log/queen.log data={preview}\n"
            + "\n[audit] unrelated append observation\n".join(records)
            + "\n[console] ERR FRAME reason=invalid-length\n[console] OK QUIT\n")


@pytest.mark.parametrize("preview", ["boot", "batch-1|batch-2|batch-3"])
def test_full_stream_proves_order_independently_of_preview(preview: str) -> None:
    verify_append_stream(stream(["batch-1", "batch-2", "batch-3"], preview))


@pytest.mark.parametrize("records", [[], ["batch-1", "batch-3"],
    ["batch-2", "batch-1", "batch-3"], ["batch-1", "batch-2", "batch-3", "batch-1"],
    ["[audit] batch-1", "[audit] batch-2", "[audit] batch-3"]])
def test_preview_cannot_hide_invalid_data(records: list[str]) -> None:
    with pytest.raises(ValueError, match="records"):
        verify_append_stream(stream(records, "batch-1|batch-2|batch-3"))


def test_incomplete_or_missing_cat_is_rejected() -> None:
    with pytest.raises(ValueError, match="unterminated"):
        verify_append_stream(
            "[console] OK CAT path=/log/queen.log data=boot\n"
            "batch-1\nbatch-2\nbatch-3\n"
        )
    with pytest.raises(ValueError, match="missing"):
        verify_append_stream("batch-1\nbatch-2\nbatch-3\n[console] OK QUIT\n")
