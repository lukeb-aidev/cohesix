// CLASSIFICATION: COMMUNITY
// Filename: capture.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

//! Capture a single JPEG frame from `/dev/video0`.
//! Falls back to a generated blank image if the device is unavailable.

use image::{codecs::jpeg::JpegEncoder, ImageBuffer, Rgb};
use std::fs;
use v4l::buffer::Type;
use v4l::io::traits::CaptureStream;
use v4l::prelude::*;

/// Capture a frame and write it to `path` in JPEG format.
pub fn capture_jpeg(path: &str) -> anyhow::Result<()> {
    if let Ok(dev) = Device::new(0) {
        let mut stream = v4l::io::mmap::Stream::new(&dev, Type::VideoCapture)?;
        if let Ok((data, _)) = stream.next() {
            fs::write(path, data)?;
            return Ok(());
        }
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
