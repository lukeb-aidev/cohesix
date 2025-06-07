#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: slm_replay_trace.py v0.1
# Date Modified: 2025-07-08
# Author: Lukas Bower

import json
from pathlib import Path
from scripts import cohtrace

trace = []
cohtrace.log_event(trace, 'spawn', 'demo')
path = Path('slm.trc')
cohtrace.write_trace(path, trace)
reloaded = cohtrace.read_trace(path)
assert reloaded == trace
