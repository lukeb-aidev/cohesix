#!/usr/bin/env python3
# CLASSIFICATION: COMMUNITY
# Filename: slm_validate_trace.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-22
# SPDX-License-Identifier: Apache-2.0
# SLM Action: validate
# Target: trace

import json
from pathlib import Path
from scripts import cohtrace

trace = []
cohtrace.log_event(trace, 'spawn', 'demo')
path = Path('slm.trc')
cohtrace.write_trace(path, trace)
reloaded = cohtrace.read_trace(path)
assert reloaded == trace
