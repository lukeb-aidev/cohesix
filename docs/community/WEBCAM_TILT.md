// CLASSIFICATION: COMMUNITY
// Filename: WEBCAM_TILT.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Webcam Tilt Service

This document describes the real-time webcam tilt demo for Cohesix. Workers capture a frame from `/dev/video0`, map the horizontal offset to a force in a Rapier beam balance simulation, and push the resulting trace to the Queen for validation. The Queen stores validation reports under `/trace/reports/`.
