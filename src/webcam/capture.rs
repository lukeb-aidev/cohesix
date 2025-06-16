// CLASSIFICATION: COMMUNITY
// Filename: capture.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-08-17
#![cfg(not(target_os = "uefi"))]

//! Capture a single JPEG frame from `/dev/video0`.
//! Falls back to a generated blank image if the device is unavailable.

use image::{codecs::jpeg::JpegEncoder, ImageBuffer, Rgb};
use std::fs;
use std::path::Path;
use crate::telemetry::telemetry::emit_kv;
use v4l::buffer::Type;
use v4l::io::traits::CaptureStream;
use v4l::prelude::*;

/// Capture a frame and write it to `path` in JPEG format.
pub fn capture_jpeg(path: &str) -> anyhow::Result<()> {
    let dev_path = std::env::var("VIDEO_DEVICE").unwrap_or_else(|_| "/dev/video0".into());
    if !Path::new(&dev_path).exists() {
        emit_kv("webcam", &[("status", "missing"), ("device", &dev_path)]);
        return write_blank_jpeg(path);
    }
    if let Ok(dev) = Device::with_path(&dev_path) {
        let mut stream = v4l::io::mmap::Stream::new(&dev, Type::VideoCapture)?;
        if let Ok((data, _)) = stream.next() {
            fs::write(path, data)?;
            return Ok(());
        }
    } else {
        emit_kv("webcam", &[("status", "open_failed"), ("device", &dev_path)]);
    }
    write_blank_jpeg(path)
}

fn write_blank_jpeg(path: &str) -> anyhow::Result<()> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(1, 1, Rgb([0, 0, 0]));
    let mut buf = Vec::new();
    JpegEncoder::new(&mut buf).encode_image(&img)?;
    fs::write(path, buf)?;
    Ok(())
}
