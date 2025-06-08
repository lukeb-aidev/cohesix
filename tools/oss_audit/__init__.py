# CLASSIFICATION: COMMUNITY
# Filename: __init__.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Convenience wrapper to run the OSS audit toolchain."""
from .scan import run_audit

__all__ = ["run_audit"]
