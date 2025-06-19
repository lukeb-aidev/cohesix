# CLASSIFICATION: COMMUNITY
# Filename: worker_inference.py v0.1
# Author: Cohesix Codex
# Date Modified: 2025-07-08
"""Webcam-based inference loop."""

import os
import cv2


def run():
    task = os.environ.get("INFER_CONF", "motion")
    cap = cv2.VideoCapture("/srv/cam0/raw")
    face_cascade = cv2.CascadeClassifier(
        cv2.data.haarcascades + "haarcascade_frontalface_default.xml"
    )
    while True:
        ret, frame = cap.read()
        if not ret:
            break
        if task == "count faces":
            gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            faces = face_cascade.detectMultiScale(gray, 1.1, 3)
            open("/srv/infer/out", "w").write(f"faces:{len(faces)}")
        else:
            open("/srv/infer/out", "w").write("motion")


if __name__ == "__main__":
    run()
