# CLASSIFICATION: COMMUNITY
# Filename: worker_inference.py v0.2
# Author: Cohesix Codex
# Date Modified: 2025-07-08
"""9P camera inference loop."""

import os
import cv2
import numpy as np


def run():
    task = os.environ.get("INFER_CONF", "motion")
    face_cascade = cv2.CascadeClassifier(
        cv2.data.haarcascades + "haarcascade_frontalface_default.xml"
    )
    frame_path = "/srv/camera/frame.jpg"
    while True:
        try:
            data = open(frame_path, "rb").read()
        except OSError:
            break
        arr = np.frombuffer(data, dtype=np.uint8)
        frame = cv2.imdecode(arr, cv2.IMREAD_COLOR)
        if frame is None:
            break
        if task == "count faces":
            gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            faces = face_cascade.detectMultiScale(gray, 1.1, 3)
            open("/srv/infer/out", "w").write(f"faces:{len(faces)}")
        else:
            open("/srv/infer/out", "w").write("motion")


if __name__ == "__main__":
    run()
