# CLASSIFICATION: COMMUNITY
# Filename: test_threat_model_cadence.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-31

from datetime import date
from tools.threat_model_scheduler import ensure_outputs, generate_reviews


def test_generate_reviews_uses_next_quarter_start():
    reviews = generate_reviews(date(2025, 1, 15), 2)
    assert len(reviews) == 2
    assert reviews[0].date == date(2025, 4, 1)
    assert reviews[1].date == date(2025, 7, 1)
    assert reviews[0].trace_id.startswith("tmr-20250401")


def test_scheduler_emits_ics_and_adr(tmp_path):
    base = tmp_path
    result = ensure_outputs(base, date(2025, 2, 10), 1)
    ics_text = result.ics_path.read_text(encoding="utf-8")
    adr_text = result.adr_paths[0].read_text(encoding="utf-8")
    trace_id = result.reviews[0].trace_id

    assert "BEGIN:VEVENT" in ics_text
    assert trace_id in ics_text
    assert trace_id in adr_text
    assert "Threat Model Review" in adr_text

    # re-running should not duplicate trace IDs
    result_repeat = ensure_outputs(base, date(2025, 2, 10), 1)
    repeat_text = result_repeat.adr_paths[0].read_text(encoding="utf-8")
    assert repeat_text.count(trace_id) == 1
