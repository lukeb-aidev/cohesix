// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! HID (Human Interface Device) support for keyboards and mice.
//!
//! Provides structures and functions for handling USB HID devices
//! using the Boot Protocol, suitable for early system initialization.
//!
//! # Features
//!
//! - Boot Protocol keyboard and mouse support
//! - Scancode to ASCII conversion
//! - Modifier key detection
//! - LED control for keyboards

use crate::{
    Dma, Result, UsbError,
    desc::{
        EndpointDesc, InterfaceDesc, SetupPacket, class, desc_type, ep_type, hid_protocol,
        hid_subclass,
    },
    dev::UsbDevice,
    ring::{PhysMem, completion},
};

use alloc::{sync::Arc, vec::Vec};
use core::{
    hint::spin_loop,
    ptr,
    sync::atomic::{Ordering, compiler_fence},
};
use spin::Mutex;

const BOOT_KEYBOARD_DECODE_FALLBACK_THRESHOLD: u8 = 4;
const UBOOT_BOOT_KEYBOARD_IDLE_DURATION: u8 = 10;
const BOOT_KEYBOARD_REPORT_LEN: usize = core::mem::size_of::<KeyboardReport>();
const FLEXIBLE_KEYBOARD_REPORT_MIN_LEN: usize = 4;
const HID_LED_CONTROL_WAIT_SPINS: usize = 2_000_000;
const HID_CONTROL_GET_REPORT_WAIT_SPINS: usize = 64_000;
const HID_CONTROL_GET_REPORT_COOLDOWN_POLLS: u8 = 7;
const HID_KEYBOARD_INTERRUPT_READ_QUEUE_DEPTH: usize = 128;
const HID_DEFAULT_INTERRUPT_READ_QUEUE_DEPTH: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueuedInterruptRead {
    trb_addr: u64,
    buffer_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyboardCompletionAction {
    ReturnReport,
    ContinuePolling,
    Error,
}

#[inline]
const fn has_report_payload(
    completion_code: u8,
    request_len: usize,
    remaining_len: u32,
    report_len: usize,
) -> bool {
    if completion_code != completion::SUCCESS && completion_code != completion::SHORT_PACKET {
        return false;
    }
    request_len.saturating_sub(remaining_len as usize) >= report_len
}

#[inline]
const fn keyboard_completion_action(
    completion_code: u8,
    payload_complete: bool,
    report_decoded: bool,
) -> KeyboardCompletionAction {
    if payload_complete && report_decoded {
        KeyboardCompletionAction::ReturnReport
    } else if completion_code == completion::SUCCESS || completion_code == completion::SHORT_PACKET
    {
        KeyboardCompletionAction::ContinuePolling
    } else {
        KeyboardCompletionAction::Error
    }
}

#[inline]
const fn keyboard_usage_code_valid(code: u8) -> bool {
    code == 0 || (code >= 0x04 && code <= 0xE7)
}

#[inline]
const fn keyboard_keys_valid(keys: &[u8; 6]) -> bool {
    let mut index = 0usize;
    while index < keys.len() {
        if !keyboard_usage_code_valid(keys[index]) {
            return false;
        }
        index += 1;
    }
    true
}

#[inline]
fn keyboard_report_has_key(report: &KeyboardReport) -> bool {
    report.keys.iter().any(|key| *key != 0)
}

#[inline]
fn keyboard_payload_prefix(payload: &[u8]) -> u64 {
    let mut value = 0u64;
    let mut index = 0usize;
    while index < payload.len() && index < core::mem::size_of::<u64>() {
        value |= u64::from(payload[index]) << (index * 8);
        index += 1;
    }
    value
}

#[inline]
fn keyboard_payload_has_nonzero_outside_boot_window(buf: &[u8], preferred_offset: usize) -> bool {
    let window_start = preferred_offset.min(buf.len());
    let window_end = window_start
        .saturating_add(BOOT_KEYBOARD_REPORT_LEN)
        .min(buf.len());
    buf.iter()
        .enumerate()
        .any(|(index, byte)| *byte != 0 && (index < window_start || index >= window_end))
}

#[inline]
fn decode_boot_keyboard_report_at(buf: &[u8], offset: usize) -> Option<KeyboardReport> {
    if offset.checked_add(core::mem::size_of::<KeyboardReport>())? > buf.len() {
        return None;
    }
    let report = KeyboardReport {
        modifiers: buf[offset],
        reserved: buf[offset + 1],
        keys: [
            buf[offset + 2],
            buf[offset + 3],
            buf[offset + 4],
            buf[offset + 5],
            buf[offset + 6],
            buf[offset + 7],
        ],
    };
    if report.reserved != 0 || !keyboard_keys_valid(&report.keys) {
        return None;
    }
    Some(report)
}

#[inline]
fn decode_compact_keyboard_report_at(buf: &[u8], offset: usize) -> Option<KeyboardReport> {
    // Some report-protocol keyboards expose `[modifiers, keys[6]]` without the
    // reserved byte used by the HID Boot protocol keyboard report.
    if offset.checked_add(7)? > buf.len() {
        return None;
    }
    let report = KeyboardReport {
        modifiers: buf[offset],
        reserved: 0,
        keys: [
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
            buf[offset + 4],
            buf[offset + 5],
            buf[offset + 6],
        ],
    };
    if !keyboard_keys_valid(&report.keys) {
        return None;
    }
    Some(report)
}

#[inline]
fn keyboard_keys_push(keys: &mut [u8; 6], count: &mut usize, usage: u8) {
    if !keyboard_usage_code_valid(usage) || keys[..*count].contains(&usage) {
        return;
    }
    if *count < keys.len() {
        keys[*count] = usage;
        *count += 1;
    }
}

#[inline]
fn decode_bitmap_keyboard_bits(
    modifiers: u8,
    bitmap: &[u8],
    usage_base: u8,
) -> Option<KeyboardReport> {
    // Require at least 16 bits of key bitmap payload to reduce false positives.
    if bitmap.len() < 2 {
        return None;
    }

    let mut report = KeyboardReport {
        modifiers,
        reserved: 0,
        keys: [0; 6],
    };
    let mut count = 0usize;
    for (byte_idx, byte) in bitmap.iter().enumerate() {
        if *byte == 0 {
            continue;
        }
        let Some(base_usage_idx) = byte_idx.checked_mul(8) else {
            break;
        };
        if base_usage_idx > u8::MAX as usize {
            break;
        }
        for bit in 0..8usize {
            if ((byte >> bit) & 1) == 0 {
                continue;
            }
            let usage_idx = base_usage_idx.saturating_add(bit);
            if usage_idx > u8::MAX as usize {
                break;
            }
            let usage = usage_base.wrapping_add(usage_idx as u8);
            if usage < usage_base {
                break;
            }
            keyboard_keys_push(&mut report.keys, &mut count, usage);
            if count >= report.keys.len() {
                return Some(report);
            }
        }
    }

    if count == 0 { None } else { Some(report) }
}

#[inline]
fn decode_bitmap_keyboard_report_at(buf: &[u8], offset: usize) -> Option<KeyboardReport> {
    // Only accept the conservative boot-like head shape:
    // [modifiers, reserved(0), bitmap...]
    // This avoids decoding arbitrary non-keyboard payloads as bitmap keys.
    if offset.checked_add(4)? > buf.len() {
        return None;
    }

    if buf[offset + 1] != 0 {
        return None;
    }

    let modifiers = buf[offset];
    decode_bitmap_keyboard_bits(modifiers, &buf[offset + 2..], 0)
}

#[inline]
fn decode_keyboard_report_payload(buf: &[u8], preferred_offset: usize) -> Option<KeyboardReport> {
    if buf.len() < 4 {
        return None;
    }

    let try_offset = |offset: usize| {
        decode_boot_keyboard_report_at(buf, offset)
            .or_else(|| decode_compact_keyboard_report_at(buf, offset))
            .or_else(|| decode_bitmap_keyboard_report_at(buf, offset))
    };

    if let Some(report) = try_offset(preferred_offset) {
        return Some(report);
    }

    for offset in 0..buf.len() {
        if offset == preferred_offset {
            continue;
        }
        if let Some(report) = try_offset(offset) {
            return Some(report);
        }
    }
    None
}

#[inline]
fn decode_keyboard_report_payload_boot_compatible(
    buf: &[u8],
    preferred_offset: usize,
) -> Option<KeyboardReport> {
    if buf.len() < BOOT_KEYBOARD_REPORT_LEN {
        return None;
    }

    let try_offset = |offset: usize| decode_boot_keyboard_report_at(buf, offset);

    if let Some(report) = try_offset(preferred_offset) {
        return Some(report);
    }

    for offset in [0usize, 1usize] {
        if offset == preferred_offset {
            continue;
        }
        if let Some(report) = try_offset(offset) {
            return Some(report);
        }
    }
    None
}

#[inline]
fn decode_keyboard_report_payload_flexible_key_report(
    buf: &[u8],
    preferred_offset: usize,
) -> Option<KeyboardReport> {
    if buf.len() < FLEXIBLE_KEYBOARD_REPORT_MIN_LEN {
        return None;
    }

    let try_offset = |offset: usize| {
        let decoders: [fn(&[u8], usize) -> Option<KeyboardReport>; 3] = [
            decode_boot_keyboard_report_at,
            decode_compact_keyboard_report_at,
            decode_bitmap_keyboard_report_at,
        ];
        for decoder in decoders {
            if let Some(report) = decoder(buf, offset) {
                if keyboard_report_has_key(&report) {
                    return Some(report);
                }
            }
        }
        None
    };

    if let Some(report) = try_offset(preferred_offset) {
        return Some(report);
    }

    for offset in 0..buf.len() {
        if offset == preferred_offset {
            continue;
        }
        if let Some(report) = try_offset(offset) {
            return Some(report);
        }
    }
    None
}

#[inline]
const fn keyboard_report_payload_min_len(
    decode_mode: KeyboardDecodeMode,
    _preferred_offset: u8,
) -> usize {
    match decode_mode {
        KeyboardDecodeMode::BootCompatible => FLEXIBLE_KEYBOARD_REPORT_MIN_LEN,
        KeyboardDecodeMode::Flexible => FLEXIBLE_KEYBOARD_REPORT_MIN_LEN,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyboardDecodeMode {
    BootCompatible,
    Flexible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyboardProtocolMode {
    Boot,
    Report,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyboardDecodeTransition {
    None,
    SwitchToReportProtocol,
    RelaxDecode,
}

#[inline]
const fn forced_keyboard_profile(
    report_id_prefixed: bool,
) -> (KeyboardProtocolMode, KeyboardDecodeMode, u8) {
    if report_id_prefixed {
        (
            KeyboardProtocolMode::Report,
            KeyboardDecodeMode::BootCompatible,
            1,
        )
    } else {
        (
            KeyboardProtocolMode::Boot,
            KeyboardDecodeMode::BootCompatible,
            0,
        )
    }
}

#[inline]
const fn keyboard_decode_transition(
    protocol_mode: KeyboardProtocolMode,
    decode_mode: KeyboardDecodeMode,
) -> KeyboardDecodeTransition {
    match decode_mode {
        KeyboardDecodeMode::Flexible => KeyboardDecodeTransition::None,
        KeyboardDecodeMode::BootCompatible => match protocol_mode {
            KeyboardProtocolMode::Boot => KeyboardDecodeTransition::SwitchToReportProtocol,
            KeyboardProtocolMode::Report => KeyboardDecodeTransition::RelaxDecode,
        },
    }
}

#[inline]
#[cfg(test)]
const fn keyboard_endpoint_id_matches(ep_num: u8, endpoint_id: u8) -> bool {
    let dci_in = (ep_num << 1) | 1;
    endpoint_id == dci_in
}

/// HID Usage Page codes.
pub mod usage_page {
    /// Generic Desktop Controls (keyboard, mouse, joystick)
    pub const GENERIC_DESKTOP: u16 = 0x01;
    /// Simulation Controls
    pub const SIMULATION: u16 = 0x02;
    /// VR Controls
    pub const VR: u16 = 0x03;
    /// Sport Controls
    pub const SPORT: u16 = 0x04;
    /// Game Controls
    pub const GAME: u16 = 0x05;
    /// Generic Device Controls
    pub const GENERIC_DEVICE: u16 = 0x06;
    /// Keyboard/Keypad
    pub const KEYBOARD: u16 = 0x07;
    /// LEDs
    pub const LED: u16 = 0x08;
    /// Button
    pub const BUTTON: u16 = 0x09;
    /// Ordinal
    pub const ORDINAL: u16 = 0x0A;
    /// Telephony
    pub const TELEPHONY: u16 = 0x0B;
    /// Consumer
    pub const CONSUMER: u16 = 0x0C;
    /// Digitizer
    pub const DIGITIZER: u16 = 0x0D;
    /// Physical Interface Device
    pub const PID: u16 = 0x0F;
    /// Unicode
    pub const UNICODE: u16 = 0x10;
    /// Alphanumeric Display
    pub const ALPHANUMERIC_DISPLAY: u16 = 0x14;
    /// Medical Instruments
    pub const MEDICAL: u16 = 0x40;
    /// Monitor Control
    pub const MONITOR_CONTROL: u16 = 0x80;
    /// Monitor Enumerated
    pub const MONITOR_ENUM: u16 = 0x81;
    /// VESA Virtual Controls
    pub const VESA_VIRTUAL: u16 = 0x82;
    /// Power Device
    pub const POWER_DEVICE: u16 = 0x84;
    /// Battery System
    pub const BATTERY: u16 = 0x85;
    /// Bar Code Scanner
    pub const BARCODE: u16 = 0x8C;
    /// Scale
    pub const SCALE: u16 = 0x8D;
    /// Magnetic Stripe Reader
    pub const MSR: u16 = 0x8E;
    /// Camera Control
    pub const CAMERA: u16 = 0x90;
    /// Arcade
    pub const ARCADE: u16 = 0x91;
    /// Vendor Defined start
    pub const VENDOR_DEFINED_START: u16 = 0xFF00;
}

/// Generic Desktop Usage IDs.
pub mod usage_desktop {
    /// Pointer
    pub const POINTER: u8 = 0x01;
    /// Mouse
    pub const MOUSE: u8 = 0x02;
    /// Joystick
    pub const JOYSTICK: u8 = 0x04;
    /// Gamepad
    pub const GAMEPAD: u8 = 0x05;
    /// Keyboard
    pub const KEYBOARD: u8 = 0x06;
    /// Keypad
    pub const KEYPAD: u8 = 0x07;
    /// Multi-axis Controller
    pub const MULTI_AXIS: u8 = 0x08;
    /// Tablet PC System Controls
    pub const TABLET_PC: u8 = 0x09;
    /// X axis
    pub const X: u8 = 0x30;
    /// Y axis
    pub const Y: u8 = 0x31;
    /// Z axis
    pub const Z: u8 = 0x32;
    /// Rx (rotation X)
    pub const RX: u8 = 0x33;
    /// Ry (rotation Y)
    pub const RY: u8 = 0x34;
    /// Rz (rotation Z)
    pub const RZ: u8 = 0x35;
    /// Slider
    pub const SLIDER: u8 = 0x36;
    /// Dial
    pub const DIAL: u8 = 0x37;
    /// Wheel
    pub const WHEEL: u8 = 0x38;
    /// Hat switch
    pub const HAT_SWITCH: u8 = 0x39;
}

/// Keyboard modifier key bits.
pub mod modifier {
    /// Left Control
    pub const LEFT_CTRL: u8 = 0x01;
    /// Left Shift
    pub const LEFT_SHIFT: u8 = 0x02;
    /// Left Alt
    pub const LEFT_ALT: u8 = 0x04;
    /// Left GUI (Windows/Command)
    pub const LEFT_GUI: u8 = 0x08;
    /// Right Control
    pub const RIGHT_CTRL: u8 = 0x10;
    /// Right Shift
    pub const RIGHT_SHIFT: u8 = 0x20;
    /// Right Alt
    pub const RIGHT_ALT: u8 = 0x40;
    /// Right GUI
    pub const RIGHT_GUI: u8 = 0x80;
    /// Any Ctrl key
    pub const CTRL: u8 = LEFT_CTRL | RIGHT_CTRL;
    /// Any Shift key
    pub const SHIFT: u8 = LEFT_SHIFT | RIGHT_SHIFT;
    /// Any Alt key
    pub const ALT: u8 = LEFT_ALT | RIGHT_ALT;
    /// Any GUI key
    pub const GUI: u8 = LEFT_GUI | RIGHT_GUI;
}

/// Keyboard LED bits (for SET_REPORT output report).
pub mod led {
    /// Num Lock LED
    pub const NUM_LOCK: u8 = 0x01;
    /// Caps Lock LED
    pub const CAPS_LOCK: u8 = 0x02;
    /// Scroll Lock LED
    pub const SCROLL_LOCK: u8 = 0x04;
    /// Compose LED
    pub const COMPOSE: u8 = 0x08;
    /// Kana LED
    pub const KANA: u8 = 0x10;
}

/// USB HID keyboard scancodes (Usage IDs from Usage Page 0x07).
pub mod scancode {
    /// No key pressed
    pub const NONE: u8 = 0x00;
    /// Error rollover (too many keys)
    pub const ERR_ROLLOVER: u8 = 0x01;
    /// POST Fail
    pub const POST_FAIL: u8 = 0x02;
    /// Undefined Error
    pub const ERR_UNDEFINED: u8 = 0x03;

    // Letters (0x04 - 0x1D)
    /// A key
    pub const A: u8 = 0x04;
    /// B key
    pub const B: u8 = 0x05;
    /// C key
    pub const C: u8 = 0x06;
    /// D key
    pub const D: u8 = 0x07;
    /// E key
    pub const E: u8 = 0x08;
    /// F key
    pub const F: u8 = 0x09;
    /// G key
    pub const G: u8 = 0x0A;
    /// H key
    pub const H: u8 = 0x0B;
    /// I key
    pub const I: u8 = 0x0C;
    /// J key
    pub const J: u8 = 0x0D;
    /// K key
    pub const K: u8 = 0x0E;
    /// L key
    pub const L: u8 = 0x0F;
    /// M key
    pub const M: u8 = 0x10;
    /// N key
    pub const N: u8 = 0x11;
    /// O key
    pub const O: u8 = 0x12;
    /// P key
    pub const P: u8 = 0x13;
    /// Q key
    pub const Q: u8 = 0x14;
    /// R key
    pub const R: u8 = 0x15;
    /// S key
    pub const S: u8 = 0x16;
    /// T key
    pub const T: u8 = 0x17;
    /// U key
    pub const U: u8 = 0x18;
    /// V key
    pub const V: u8 = 0x19;
    /// W key
    pub const W: u8 = 0x1A;
    /// X key
    pub const X: u8 = 0x1B;
    /// Y key
    pub const Y: u8 = 0x1C;
    /// Z key
    pub const Z: u8 = 0x1D;

    // Numbers (0x1E - 0x27)
    /// 1 key
    pub const N1: u8 = 0x1E;
    /// 2 key
    pub const N2: u8 = 0x1F;
    /// 3 key
    pub const N3: u8 = 0x20;
    /// 4 key
    pub const N4: u8 = 0x21;
    /// 5 key
    pub const N5: u8 = 0x22;
    /// 6 key
    pub const N6: u8 = 0x23;
    /// 7 key
    pub const N7: u8 = 0x24;
    /// 8 key
    pub const N8: u8 = 0x25;
    /// 9 key
    pub const N9: u8 = 0x26;
    /// 0 key
    pub const N0: u8 = 0x27;

    // Special keys
    /// Enter/Return
    pub const ENTER: u8 = 0x28;
    /// Escape
    pub const ESCAPE: u8 = 0x29;
    /// Backspace
    pub const BACKSPACE: u8 = 0x2A;
    /// Tab
    pub const TAB: u8 = 0x2B;
    /// Space
    pub const SPACE: u8 = 0x2C;
    /// Minus/Underscore
    pub const MINUS: u8 = 0x2D;
    /// Equal/Plus
    pub const EQUAL: u8 = 0x2E;
    /// Left Bracket
    pub const LEFT_BRACKET: u8 = 0x2F;
    /// Right Bracket
    pub const RIGHT_BRACKET: u8 = 0x30;
    /// Backslash
    pub const BACKSLASH: u8 = 0x31;
    /// Non-US Hash
    pub const NON_US_HASH: u8 = 0x32;
    /// Semicolon
    pub const SEMICOLON: u8 = 0x33;
    /// Apostrophe/Quote
    pub const APOSTROPHE: u8 = 0x34;
    /// Grave/Tilde
    pub const GRAVE: u8 = 0x35;
    /// Comma
    pub const COMMA: u8 = 0x36;
    /// Period/Dot
    pub const PERIOD: u8 = 0x37;
    /// Slash
    pub const SLASH: u8 = 0x38;
    /// Caps Lock
    pub const CAPS_LOCK: u8 = 0x39;

    // Function keys (0x3A - 0x45)
    /// F1
    pub const F1: u8 = 0x3A;
    /// F2
    pub const F2: u8 = 0x3B;
    /// F3
    pub const F3: u8 = 0x3C;
    /// F4
    pub const F4: u8 = 0x3D;
    /// F5
    pub const F5: u8 = 0x3E;
    /// F6
    pub const F6: u8 = 0x3F;
    /// F7
    pub const F7: u8 = 0x40;
    /// F8
    pub const F8: u8 = 0x41;
    /// F9
    pub const F9: u8 = 0x42;
    /// F10
    pub const F10: u8 = 0x43;
    /// F11
    pub const F11: u8 = 0x44;
    /// F12
    pub const F12: u8 = 0x45;

    // Control keys
    /// Print Screen
    pub const PRINT_SCREEN: u8 = 0x46;
    /// Scroll Lock
    pub const SCROLL_LOCK: u8 = 0x47;
    /// Pause
    pub const PAUSE: u8 = 0x48;
    /// Insert
    pub const INSERT: u8 = 0x49;
    /// Home
    pub const HOME: u8 = 0x4A;
    /// Page Up
    pub const PAGE_UP: u8 = 0x4B;
    /// Delete
    pub const DELETE: u8 = 0x4C;
    /// End
    pub const END: u8 = 0x4D;
    /// Page Down
    pub const PAGE_DOWN: u8 = 0x4E;
    /// Right Arrow
    pub const RIGHT_ARROW: u8 = 0x4F;
    /// Left Arrow
    pub const LEFT_ARROW: u8 = 0x50;
    /// Down Arrow
    pub const DOWN_ARROW: u8 = 0x51;
    /// Up Arrow
    pub const UP_ARROW: u8 = 0x52;
    /// Num Lock
    pub const NUM_LOCK: u8 = 0x53;

    // Keypad
    /// Keypad /
    pub const KP_DIVIDE: u8 = 0x54;
    /// Keypad *
    pub const KP_MULTIPLY: u8 = 0x55;
    /// Keypad -
    pub const KP_MINUS: u8 = 0x56;
    /// Keypad +
    pub const KP_PLUS: u8 = 0x57;
    /// Keypad Enter
    pub const KP_ENTER: u8 = 0x58;
    /// Keypad 1/End
    pub const KP_1: u8 = 0x59;
    /// Keypad 2/Down
    pub const KP_2: u8 = 0x5A;
    /// Keypad 3/PgDn
    pub const KP_3: u8 = 0x5B;
    /// Keypad 4/Left
    pub const KP_4: u8 = 0x5C;
    /// Keypad 5
    pub const KP_5: u8 = 0x5D;
    /// Keypad 6/Right
    pub const KP_6: u8 = 0x5E;
    /// Keypad 7/Home
    pub const KP_7: u8 = 0x5F;
    /// Keypad 8/Up
    pub const KP_8: u8 = 0x60;
    /// Keypad 9/PgUp
    pub const KP_9: u8 = 0x61;
    /// Keypad 0/Ins
    pub const KP_0: u8 = 0x62;
    /// Keypad ./Del
    pub const KP_DECIMAL: u8 = 0x63;

    // Additional keys
    /// Non-US Backslash
    pub const NON_US_BACKSLASH: u8 = 0x64;
    /// Application/Menu
    pub const APPLICATION: u8 = 0x65;
    /// Power
    pub const POWER: u8 = 0x66;
    /// Keypad =
    pub const KP_EQUAL: u8 = 0x67;

    // Extended function keys
    /// F13
    pub const F13: u8 = 0x68;
    /// F14
    pub const F14: u8 = 0x69;
    /// F15
    pub const F15: u8 = 0x6A;
    /// F16
    pub const F16: u8 = 0x6B;
    /// F17
    pub const F17: u8 = 0x6C;
    /// F18
    pub const F18: u8 = 0x6D;
    /// F19
    pub const F19: u8 = 0x6E;
    /// F20
    pub const F20: u8 = 0x6F;
    /// F21
    pub const F21: u8 = 0x70;
    /// F22
    pub const F22: u8 = 0x71;
    /// F23
    pub const F23: u8 = 0x72;
    /// F24
    pub const F24: u8 = 0x73;

    // Modifier keys (these don't appear in the keys array, only in modifiers byte)
    /// Left Control
    pub const LEFT_CTRL: u8 = 0xE0;
    /// Left Shift
    pub const LEFT_SHIFT: u8 = 0xE1;
    /// Left Alt
    pub const LEFT_ALT: u8 = 0xE2;
    /// Left GUI
    pub const LEFT_GUI: u8 = 0xE3;
    /// Right Control
    pub const RIGHT_CTRL: u8 = 0xE4;
    /// Right Shift
    pub const RIGHT_SHIFT: u8 = 0xE5;
    /// Right Alt
    pub const RIGHT_ALT: u8 = 0xE6;
    /// Right GUI
    pub const RIGHT_GUI: u8 = 0xE7;
}

/// HID report types for GET_REPORT/SET_REPORT requests.
pub mod report_type {
    /// Input report
    pub const INPUT: u8 = 1;
    /// Output report
    pub const OUTPUT: u8 = 2;
    /// Feature report
    pub const FEATURE: u8 = 3;
}

/// HID Boot Protocol Keyboard Report (8 bytes).
///
/// Standard keyboard report format for Boot Protocol keyboards.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyboardReport {
    /// Modifier keys bitmap (Ctrl, Shift, Alt, GUI for left and right)
    pub modifiers: u8,
    /// Reserved byte (always 0)
    pub reserved: u8,
    /// Up to 6 simultaneous key scancodes
    pub keys: [u8; 6],
}

impl KeyboardReport {
    /// Returns true if either Ctrl key is pressed.
    pub fn ctrl(&self) -> bool {
        (self.modifiers & 0x11) != 0
    }

    /// Returns true if either Shift key is pressed.
    pub fn shift(&self) -> bool {
        (self.modifiers & 0x22) != 0
    }

    /// Returns true if either Alt key is pressed.
    pub fn alt(&self) -> bool {
        (self.modifiers & 0x44) != 0
    }

    /// Returns true if either GUI (Windows/Command) key is pressed.
    pub fn gui(&self) -> bool {
        (self.modifiers & 0x88) != 0
    }
}

/// HID Boot Protocol Mouse Report (3 bytes).
///
/// Standard mouse report format for Boot Protocol mice.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseReport {
    /// Button state bitmap
    pub buttons: u8,
    /// X-axis relative movement (-127 to 127)
    pub x: i8,
    /// Y-axis relative movement (-127 to 127)
    pub y: i8,
}

impl MouseReport {
    /// Returns true if the left button is pressed.
    pub fn left(&self) -> bool {
        (self.buttons & 0x01) != 0
    }

    /// Returns true if the right button is pressed.
    pub fn right(&self) -> bool {
        (self.buttons & 0x02) != 0
    }

    /// Returns true if the middle button is pressed.
    pub fn middle(&self) -> bool {
        (self.buttons & 0x04) != 0
    }
}

/// HID device type classification.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HidType {
    /// Boot Protocol Keyboard
    Keyboard,
    /// Boot Protocol Mouse
    Mouse,
    /// Other HID device (not Boot Protocol compatible)
    Other,
}

#[inline]
const fn interrupt_read_queue_depth(hid_type: HidType) -> usize {
    match hid_type {
        HidType::Keyboard => HID_KEYBOARD_INTERRUPT_READ_QUEUE_DEPTH,
        HidType::Mouse | HidType::Other => HID_DEFAULT_INTERRUPT_READ_QUEUE_DEPTH,
    }
}

/// HID Device wrapper.
///
/// Provides high-level interface for reading input from HID keyboards
/// and mice using the Boot Protocol.
pub struct HidDevice<H: Dma> {
    device: Arc<UsbDevice<H>>,
    hid_type: HidType,
    interface: u8,
    ep_in: u8,
    ep_max_packet: u16,
    keyboard_decode_failures: u8,
    keyboard_protocol_mode: KeyboardProtocolMode,
    keyboard_decode_mode: KeyboardDecodeMode,
    keyboard_report_offset: u8,
    report_bufs: Vec<PhysMem<H>>,
    queued_reads: Mutex<Vec<QueuedInterruptRead>>,
    control_report_buf: PhysMem<H>,
    led_report_buf: PhysMem<H>,
    keyboard_control_poll_disabled: bool,
    keyboard_control_poll_cooldown: u8,
}

impl<H: Dma> HidDevice<H> {
    /// Try to create a HID device from an interface descriptor
    pub fn from_interface(
        device: Arc<UsbDevice<H>>,
        iface: &InterfaceDesc,
        ep_in: &EndpointDesc,
    ) -> Result<Self> {
        if iface.interface_class != class::HID {
            return Err(UsbError::NotSupported);
        }

        let hid_type = if iface.interface_subclass == hid_subclass::BOOT {
            match iface.interface_protocol {
                hid_protocol::KEYBOARD => HidType::Keyboard,
                hid_protocol::MOUSE => HidType::Mouse,
                _ => HidType::Other,
            }
        } else {
            HidType::Other
        };

        // Configure the interrupt endpoint
        device.configure_endpoint(ep_in)?;

        // Allocate report buffers (64-byte alignment for DMA). Pi 4 keyboard
        // polling runs without xHCI interrupts, so keep a deep queue of
        // interrupt-IN reads armed to cover bounded console-output stalls.
        let host = device.ctrl().host();
        let read_queue_depth = interrupt_read_queue_depth(hid_type);
        let mut report_bufs = Vec::with_capacity(read_queue_depth);
        for _ in 0..read_queue_depth {
            match PhysMem::alloc(host, ep_in.max_packet_size as usize, 64) {
                Ok(buf) => report_bufs.push(buf),
                Err(err) => {
                    while let Some(buf) = report_bufs.pop() {
                        buf.free(host);
                    }
                    return Err(err);
                }
            }
        }
        let led_report_buf = match PhysMem::alloc(host, 1, 64) {
            Ok(buf) => buf,
            Err(err) => {
                while let Some(buf) = report_bufs.pop() {
                    buf.free(host);
                }
                return Err(err);
            }
        };
        let control_report_buf = match PhysMem::alloc(host, ep_in.max_packet_size as usize, 64) {
            Ok(buf) => buf,
            Err(err) => {
                while let Some(buf) = report_bufs.pop() {
                    buf.free(host);
                }
                led_report_buf.free(host);
                return Err(err);
            }
        };
        let queued_reads = Mutex::new(Vec::with_capacity(read_queue_depth));

        let hid = Self {
            device,
            hid_type,
            interface: iface.interface_number,
            ep_in: ep_in.number(),
            ep_max_packet: ep_in.max_packet_size,
            keyboard_decode_failures: 0,
            keyboard_protocol_mode: if hid_type == HidType::Keyboard {
                KeyboardProtocolMode::Boot
            } else {
                KeyboardProtocolMode::Report
            },
            keyboard_decode_mode: if hid_type == HidType::Keyboard {
                KeyboardDecodeMode::BootCompatible
            } else {
                KeyboardDecodeMode::Flexible
            },
            keyboard_report_offset: 0,
            report_bufs,
            queued_reads,
            control_report_buf,
            led_report_buf,
            keyboard_control_poll_disabled: false,
            keyboard_control_poll_cooldown: 0,
        };

        // Set boot protocol for boot devices
        if iface.interface_subclass == hid_subclass::BOOT {
            hid.set_protocol(0)?; // Boot protocol
        }

        let idle_duration = if hid_type == HidType::Keyboard {
            UBOOT_BOOT_KEYBOARD_IDLE_DURATION
        } else {
            0
        };
        let _ = hid.set_idle(idle_duration, 0);

        Ok(hid)
    }

    /// Set HID protocol (0 = Boot, 1 = Report)
    pub fn set_protocol(&self, protocol: u8) -> Result<()> {
        let setup = SetupPacket::set_protocol(self.interface, protocol);
        self.device.control_transfer(&setup, None)?;
        Ok(())
    }

    /// Set idle rate
    pub fn set_idle(&self, duration: u8, report_id: u8) -> Result<()> {
        let setup = SetupPacket::set_idle(self.interface, duration, report_id);
        self.device.control_transfer(&setup, None)?;
        Ok(())
    }

    /// Sets the keyboard LEDs (Num Lock, Caps Lock, Scroll Lock).
    ///
    /// Only applicable to keyboard devices. Use the `led` module constants
    /// to construct the LED bitmap.
    pub fn set_leds(&self, leds: u8) -> Result<()> {
        if self.hid_type != HidType::Keyboard {
            return Err(UsbError::NotSupported);
        }

        let leds = leds & (led::NUM_LOCK | led::CAPS_LOCK | led::SCROLL_LOCK);
        // SAFETY: `led_report_buf` is a dedicated one-byte DMA buffer owned by
        // this HID device, allocated before any Pi 4 local-seat DMA seal.
        unsafe {
            self.led_report_buf.as_ptr::<u8>().write_volatile(leds);
        }
        compiler_fence(Ordering::Release);

        let setup = SetupPacket::hid_set_report(self.interface, report_type::OUTPUT, 0, 1);
        self.device.control_transfer_out_physmem_with_wait_spins(
            &setup,
            &self.led_report_buf,
            0,
            1,
            HID_LED_CONTROL_WAIT_SPINS,
        )?;
        Ok(())
    }

    /// Gets the current protocol (0 = Boot, 1 = Report).
    pub fn get_protocol(&self) -> Result<u8> {
        let setup = SetupPacket::hid_get_protocol(self.interface);
        let mut buf = [0u8; 1];
        self.device.control_transfer(&setup, Some(&mut buf))?;
        Ok(buf[0])
    }

    /// Gets the current idle rate for a report ID.
    pub fn get_idle(&self, report_id: u8) -> Result<u8> {
        let setup = SetupPacket::hid_get_idle(self.interface, report_id);
        let mut buf = [0u8; 1];
        self.device.control_transfer(&setup, Some(&mut buf))?;
        Ok(buf[0])
    }

    /// Queue a read from the interrupt endpoint
    pub fn queue_read(&self) -> Result<()> {
        self.queue_reads_to_depth(1).map(|_| ())
    }

    fn queue_reads_to_depth(&self, target_depth: usize) -> Result<usize> {
        let target_depth = target_depth.min(self.report_bufs.len());
        let mut queued_reads = self.queued_reads.lock();
        let mut queued = 0usize;
        while queued_reads.len() < target_depth {
            let Some(buffer_index) = self.next_free_report_buffer_index(&queued_reads) else {
                break;
            };
            let trb_addr = self.device.queue_transfer(
                self.ep_in,
                true,
                &self.report_bufs[buffer_index],
                self.ep_max_packet as usize,
            )? & !0x0f;
            queued_reads.push(QueuedInterruptRead {
                trb_addr,
                buffer_index,
            });
            queued = queued.saturating_add(1);
        }
        Ok(queued)
    }

    fn next_free_report_buffer_index(&self, queued_reads: &[QueuedInterruptRead]) -> Option<usize> {
        (0..self.report_bufs.len())
            .find(|index| !queued_reads.iter().any(|queued| queued.buffer_index == *index))
    }

    fn complete_interrupt_read(&self, trb_addr: u64) -> Option<usize> {
        let trb_addr = trb_addr & !0x0f;
        let mut queued_reads = self.queued_reads.lock();
        let index = queued_reads
            .iter()
            .position(|queued| queued.trb_addr == trb_addr)?;
        Some(queued_reads.remove(index).buffer_index)
    }

    fn arm_keyboard_reads(&self) -> Result<usize> {
        self.queue_reads_to_depth(interrupt_read_queue_depth(self.hid_type))
    }

    #[inline]
    const fn expected_in_endpoint_id(&self) -> u8 {
        (self.ep_in << 1) | 1
    }

    #[inline]
    const fn accepted_in_endpoint_ids(&self) -> [u8; 1] {
        [self.expected_in_endpoint_id()]
    }

    fn reset_keyboard_profile(
        &mut self,
        protocol_mode: KeyboardProtocolMode,
        decode_mode: KeyboardDecodeMode,
        report_offset: u8,
    ) {
        self.keyboard_protocol_mode = protocol_mode;
        self.keyboard_decode_mode = decode_mode;
        self.keyboard_report_offset = report_offset;
        self.keyboard_decode_failures = 0;
    }

    fn advance_keyboard_decode_profile(&mut self) -> bool {
        match keyboard_decode_transition(self.keyboard_protocol_mode, self.keyboard_decode_mode) {
            KeyboardDecodeTransition::None => {
                self.keyboard_decode_failures = 0;
                false
            }
            KeyboardDecodeTransition::RelaxDecode => {
                self.keyboard_decode_mode = KeyboardDecodeMode::Flexible;
                self.keyboard_decode_failures = 0;
                true
            }
            KeyboardDecodeTransition::SwitchToReportProtocol => {
                if self.set_protocol(1).is_ok() {
                    self.keyboard_protocol_mode = KeyboardProtocolMode::Report;
                    self.keyboard_decode_failures = 0;
                    false
                } else {
                    self.keyboard_decode_mode = KeyboardDecodeMode::Flexible;
                    self.keyboard_decode_failures = 0;
                    true
                }
            }
        }
    }

    fn decode_keyboard_payload_current(&mut self, payload: &[u8]) -> Option<KeyboardReport> {
        let mut report = match self.keyboard_decode_mode {
            KeyboardDecodeMode::BootCompatible => decode_keyboard_report_payload_boot_compatible(
                payload,
                usize::from(self.keyboard_report_offset),
            ),
            KeyboardDecodeMode::Flexible => {
                decode_keyboard_report_payload(payload, usize::from(self.keyboard_report_offset))
            }
        };
        if let Some(decoded) = report {
            if self.keyboard_decode_mode == KeyboardDecodeMode::BootCompatible
                && !keyboard_report_has_key(&decoded)
                && keyboard_payload_has_nonzero_outside_boot_window(
                    payload,
                    usize::from(self.keyboard_report_offset),
                )
                && let Some(flexible_report) = decode_keyboard_report_payload_flexible_key_report(
                    payload,
                    usize::from(self.keyboard_report_offset),
                )
            {
                self.keyboard_decode_mode = KeyboardDecodeMode::Flexible;
                self.keyboard_decode_failures = 0;
                report = Some(flexible_report);
            }
        }
        if report.is_none() && self.keyboard_decode_mode == KeyboardDecodeMode::BootCompatible {
            self.keyboard_decode_failures = self.keyboard_decode_failures.saturating_add(1);
            if self.keyboard_decode_failures >= BOOT_KEYBOARD_DECODE_FALLBACK_THRESHOLD
                && self.advance_keyboard_decode_profile()
            {
                report = match self.keyboard_decode_mode {
                    KeyboardDecodeMode::BootCompatible => {
                        decode_keyboard_report_payload_boot_compatible(
                            payload,
                            usize::from(self.keyboard_report_offset),
                        )
                    }
                    KeyboardDecodeMode::Flexible => {
                        decode_keyboard_report_payload(payload, usize::from(self.keyboard_report_offset))
                    }
                };
            }
        }
        if report.is_some() {
            self.keyboard_decode_failures = 0;
        }
        report
    }

    fn poll_keyboard_control_report(&mut self) -> Result<Option<KeyboardReport>> {
        if self.keyboard_control_poll_disabled {
            return Ok(None);
        }
        if self.keyboard_control_poll_cooldown != 0 {
            self.keyboard_control_poll_cooldown =
                self.keyboard_control_poll_cooldown.saturating_sub(1);
            return Ok(None);
        }
        self.keyboard_control_poll_cooldown = HID_CONTROL_GET_REPORT_COOLDOWN_POLLS;
        let request_len = (self.ep_max_packet as usize).min(self.control_report_buf.size());
        let report_min_len =
            keyboard_report_payload_min_len(self.keyboard_decode_mode, self.keyboard_report_offset);
        if request_len < report_min_len {
            self.keyboard_control_poll_disabled = true;
            return Ok(None);
        }
        // SAFETY: `control_report_buf` is a HID-owned DMA buffer and the full
        // bounded request region is valid for byte writes before device ownership.
        unsafe {
            core::ptr::write_bytes(self.control_report_buf.as_ptr::<u8>(), 0, request_len);
        }
        let setup =
            SetupPacket::hid_get_report(self.interface, report_type::INPUT, 0, request_len as u16);
        let transferred = match self.device.control_transfer_in_physmem_with_wait_spins(
            &setup,
            &self.control_report_buf,
            0,
            request_len,
            HID_CONTROL_GET_REPORT_WAIT_SPINS,
        ) {
            Ok(transferred) => transferred,
            Err(err) => {
                self.keyboard_control_poll_disabled = true;
                self.device.ctrl().emit_diag(
                    0x0419,
                    ((self.device.slot_id() as u64) << 32) | u64::from(self.expected_in_endpoint_id()),
                    request_len as u64,
                    match err {
                        UsbError::Stall => 1,
                        UsbError::Timeout => 2,
                        _ => 3,
                    },
                );
                return Ok(None);
            }
        };
        if transferred < report_min_len {
            return Ok(None);
        }
        // SAFETY: the control transfer synchronized the completed prefix for CPU
        // access, and `transferred` is capped to the preallocated DMA buffer.
        let payload = unsafe {
            core::slice::from_raw_parts(
                self.control_report_buf.as_ptr::<u8>(),
                transferred.min(request_len),
            )
        };
        if let Some(report) = self.decode_keyboard_payload_current(payload) {
            self.device.ctrl().emit_diag(
                0x0418,
                ((self.device.slot_id() as u64) << 32) | u64::from(self.expected_in_endpoint_id()),
                transferred as u64,
                keyboard_payload_prefix(payload),
            );
            return Ok(Some(report));
        }
        Ok(None)
    }

    /// Poll for keyboard report (non-blocking) with transfer re-queue errors surfaced.
    pub fn poll_keyboard_checked(&mut self) -> Result<Option<KeyboardReport>> {
        if self.hid_type != HidType::Keyboard {
            return Ok(None);
        }
        self.arm_keyboard_reads()?;

        // Drain a bounded number of events so unrelated hub chatter does not
        // indefinitely starve keyboard transfer completions.
        const HID_EVENT_BUDGET: usize = 128;
        let endpoint_ids = self.accepted_in_endpoint_ids();
        for _ in 0..HID_EVENT_BUDGET {
            let Some(evt) = self
                .device
                .ctrl()
                .poll_transfer_event_for_slot_endpoint_ids(self.device.slot_id(), &endpoint_ids)
            else {
                return self.poll_keyboard_control_report();
            };

            let code = evt.completion_code();
            let Some(buffer_index) = self.complete_interrupt_read(evt.param) else {
                self.device.ctrl().emit_diag(
                    0x0417,
                    ((self.device.slot_id() as u64) << 32) | u64::from(evt.endpoint_id()),
                    evt.param,
                    self.queued_reads.lock().len() as u64,
                );
                self.arm_keyboard_reads()?;
                continue;
            };
            let report_buf = &self.report_bufs[buffer_index];
            let payload_len =
                (self.ep_max_packet as usize).saturating_sub(evt.transfer_length() as usize);
            self.device.ctrl().emit_diag(
                0x03c0,
                ((self.device.slot_id() as u64) << 32) | u64::from(evt.endpoint_id()),
                ((code as u64) << 32) | payload_len as u64,
                ((self.keyboard_protocol_mode as u64) << 32)
                    | ((self.keyboard_decode_mode as u64) << 16)
                    | u64::from(self.keyboard_report_offset),
            );
            let report_min_len = keyboard_report_payload_min_len(
                self.keyboard_decode_mode,
                self.keyboard_report_offset,
            );
            if has_report_payload(
                code,
                self.ep_max_packet as usize,
                evt.transfer_length(),
                report_min_len,
            ) {
                report_buf.sync_for_cpu_range(
                    self.device.ctrl().host(),
                    0,
                    payload_len.min(self.ep_max_packet as usize),
                    "xhci-hid-report-buffer",
                )?;
                // SAFETY: report_buf is the DMA buffer backing this submitted interrupt-IN
                // transfer, and payload_len is clamped to the allocated endpoint packet size.
                let payload = unsafe {
                    core::slice::from_raw_parts(
                        report_buf.as_ptr::<u8>(),
                        payload_len.min(self.ep_max_packet as usize),
                    )
                };
                let decode_mode_before = self.keyboard_decode_mode;
                let report = self.decode_keyboard_payload_current(payload);
                if decode_mode_before != self.keyboard_decode_mode
                    && self.keyboard_decode_mode == KeyboardDecodeMode::Flexible
                {
                    self.device.ctrl().emit_diag(
                        0x0416,
                        ((self.device.slot_id() as u64) << 32) | u64::from(evt.endpoint_id()),
                        ((code as u64) << 32) | payload_len as u64,
                        keyboard_payload_prefix(payload),
                    );
                }
                if let Some(report) = report {
                    debug_assert_eq!(
                        keyboard_completion_action(code, true, true),
                        KeyboardCompletionAction::ReturnReport
                    );
                    self.arm_keyboard_reads()?;
                    return Ok(Some(report));
                }
                self.device.ctrl().emit_diag(
                    0x03c1,
                    ((self.device.slot_id() as u64) << 32) | u64::from(evt.endpoint_id()),
                    ((code as u64) << 32) | payload_len as u64,
                    ((self.keyboard_decode_failures as u64) << 32)
                        | ((self.keyboard_protocol_mode as u64) << 16)
                        | (self.keyboard_decode_mode as u64),
                );
                self.arm_keyboard_reads()?;
                debug_assert_eq!(
                    keyboard_completion_action(code, true, false),
                    KeyboardCompletionAction::ContinuePolling
                );
                continue;
            }
            match keyboard_completion_action(code, false, false) {
                KeyboardCompletionAction::ContinuePolling => {
                    // Ignore zero/partial payload completions and keep polling.
                    self.device.ctrl().emit_diag(
                        0x03c2,
                        ((self.device.slot_id() as u64) << 32) | u64::from(evt.endpoint_id()),
                        ((code as u64) << 32) | payload_len as u64,
                        self.ep_max_packet as u64,
                    );
                    self.arm_keyboard_reads()?;
                    continue;
                }
                KeyboardCompletionAction::ReturnReport => unreachable!(),
                KeyboardCompletionAction::Error => {}
            }
            let _ = self.arm_keyboard_reads();
            self.device.ctrl().emit_diag(
                0x03c3,
                ((self.device.slot_id() as u64) << 32) | u64::from(evt.endpoint_id()),
                ((code as u64) << 32) | payload_len as u64,
                self.ep_max_packet as u64,
            );
            return Err(UsbError::XferFail(code));
        }
        Ok(None)
    }

    /// Poll for keyboard report (non-blocking).
    pub fn poll_keyboard(&mut self) -> Option<KeyboardReport> {
        self.poll_keyboard_checked().ok().flatten()
    }

    /// Poll for mouse report (non-blocking)
    pub fn poll_mouse(&mut self) -> Option<MouseReport> {
        if self.hid_type != HidType::Mouse {
            return None;
        }
        if self.queue_reads_to_depth(1).is_err() {
            return None;
        }

        const HID_EVENT_BUDGET: usize = 128;
        let endpoint_ids = self.accepted_in_endpoint_ids();
        for _ in 0..HID_EVENT_BUDGET {
            let Some(evt) = self
                .device
                .ctrl()
                .poll_transfer_event_for_slot_endpoint_ids(self.device.slot_id(), &endpoint_ids)
            else {
                return None;
            };
            let code = evt.completion_code();
            let Some(buffer_index) = self.complete_interrupt_read(evt.param) else {
                let _ = self.queue_reads_to_depth(1);
                continue;
            };
            let report_buf = &self.report_bufs[buffer_index];
            if has_report_payload(
                code,
                self.ep_max_packet as usize,
                evt.transfer_length(),
                core::mem::size_of::<MouseReport>(),
            ) {
                let payload_len =
                    (self.ep_max_packet as usize).saturating_sub(evt.transfer_length() as usize);
                if report_buf
                    .sync_for_cpu_range(
                        self.device.ctrl().host(),
                        0,
                        payload_len.min(self.ep_max_packet as usize),
                        "xhci-hid-report-buffer",
                    )
                    .is_err()
                {
                    let _ = self.queue_reads_to_depth(1);
                    return None;
                }
                // SAFETY: `has_report_payload` proved the completed transfer
                // contains a full boot-protocol mouse report in `report_buf`.
                let report = unsafe { *(report_buf.as_ptr::<MouseReport>()) };
                let _ = self.queue_reads_to_depth(1);
                return Some(report);
            }
            if code == completion::SUCCESS || code == completion::SHORT_PACKET {
                // Ignore zero/partial payload completions and keep polling.
                let _ = self.queue_reads_to_depth(1);
                return None;
            }
            let _ = self.queue_reads_to_depth(1);
            return None;
        }
        None
    }

    /// Blocking read for keyboard
    pub fn read_keyboard(&mut self) -> Result<KeyboardReport> {
        if self.hid_type != HidType::Keyboard {
            return Err(UsbError::NotSupported);
        }

        self.queue_read()?;

        loop {
            if let Some(report) = self.poll_keyboard_checked()? {
                return Ok(report);
            }
            spin_loop();
        }
    }

    /// Blocking read for mouse
    pub fn read_mouse(&mut self) -> Result<MouseReport> {
        if self.hid_type != HidType::Mouse {
            return Err(UsbError::NotSupported);
        }

        self.queue_read()?;

        loop {
            if let Some(report) = self.poll_mouse() {
                return Ok(report);
            }
            spin_loop();
        }
    }

    /// Returns the HID device type.
    pub fn hid_type(&self) -> HidType {
        self.hid_type
    }

    /// Forces keyboard-mode report handling for compatibility paths that
    /// have already switched the interface into HID Boot protocol.
    ///
    /// This is intended for callers that discover keyboard-capable HID
    /// interfaces that do not advertise Boot keyboard protocol in
    /// `bInterfaceProtocol` but still accept `SET_PROTOCOL(BOOT)`.
    pub fn into_forced_keyboard(mut self) -> Self {
        self.hid_type = HidType::Keyboard;
        let (protocol_mode, decode_mode, report_offset) = forced_keyboard_profile(false);
        self.reset_keyboard_profile(protocol_mode, decode_mode, report_offset);
        self
    }

    /// Forces keyboard-mode handling with a one-byte report-ID prefix.
    ///
    /// Some non-boot HID keyboards keep report protocol layouts where input
    /// reports are `[report_id, modifiers, reserved, keys[6], ...]`.
    pub fn into_forced_keyboard_with_report_id(mut self) -> Self {
        self.hid_type = HidType::Keyboard;
        let (protocol_mode, decode_mode, report_offset) = forced_keyboard_profile(true);
        self.reset_keyboard_profile(protocol_mode, decode_mode, report_offset);
        self
    }

    /// Returns the interface number.
    pub fn interface(&self) -> u8 {
        self.interface
    }

    /// Returns a reference to the underlying USB device.
    pub fn device(&self) -> &Arc<UsbDevice<H>> {
        &self.device
    }
}

impl<H: Dma> Drop for HidDevice<H> {
    fn drop(&mut self) {
        let host = self.device.ctrl().host();
        // Note: PhysMem does not auto-free, so HID-owned DMA buffers are
        // returned explicitly during device teardown.
        while let Some(buf) = self.report_bufs.pop() {
            buf.free(host);
        }
        // SAFETY: `control_report_buf` was allocated from this DMA host with the
        // same size/alignment and is freed exactly once during `HidDevice` drop.
        unsafe {
            host.free(
                self.control_report_buf.virt(),
                self.control_report_buf.size(),
                self.control_report_buf.align(),
            );
        }
        // SAFETY: `led_report_buf` was allocated from this DMA host with the
        // same size/alignment and is freed exactly once during `HidDevice` drop.
        unsafe {
            host.free(
                self.led_report_buf.virt(),
                self.led_report_buf.size(),
                self.led_report_buf.align(),
            );
        }
    }
}

/// USB HID scancode to ASCII conversion (US keyboard layout)
pub fn scancode_to_ascii(scancode: u8, shift: bool) -> Option<char> {
    const NORMAL: &[u8] = b"\0\0\0\0abcdefghijklmnopqrstuvwxyz1234567890\n\x1b\x08\t -=[]\\#;'`,./";
    const SHIFTED: &[u8] =
        b"\0\0\0\0ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*()\n\x1b\x08\t _+{}|~:\"~<>?";

    let table = if shift { SHIFTED } else { NORMAL };

    if (scancode as usize) < table.len() {
        let c = table[scancode as usize];
        if c != 0 { Some(c as char) } else { None }
    } else {
        None
    }
}

/// Parse configuration descriptor to find HID interfaces
pub fn find_hid_interfaces(config_data: &[u8]) -> alloc::vec::Vec<(InterfaceDesc, EndpointDesc)> {
    let mut result = alloc::vec::Vec::new();
    let mut offset = 0;
    let mut current_iface: Option<InterfaceDesc> = None;

    while offset + 2 <= config_data.len() {
        let len = config_data[offset] as usize;
        let dtype = config_data[offset + 1];

        if len == 0 || offset + len > config_data.len() {
            break;
        }

        match dtype {
            desc_type::INTERFACE if len >= 9 => {
                // SAFETY: Descriptor data can be unaligned inside the raw configuration blob.
                let iface = unsafe {
                    ptr::read_unaligned(config_data.as_ptr().add(offset) as *const InterfaceDesc)
                };
                if iface.interface_class == class::HID {
                    current_iface = Some(iface);
                } else {
                    current_iface = None;
                }
            }
            desc_type::ENDPOINT if len >= 7 => {
                if let Some(iface) = current_iface {
                    // SAFETY: Descriptor data can be unaligned inside the raw configuration blob.
                    let ep = unsafe {
                        ptr::read_unaligned(config_data.as_ptr().add(offset) as *const EndpointDesc)
                    };
                    // Only interested in Interrupt IN endpoints
                    if ep.is_in() && ep.transfer_type() == ep_type::INTERRUPT {
                        result.push((iface, ep));
                        current_iface = None;
                    }
                }
            }
            _ => {}
        }

        offset += len;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{
        KeyboardCompletionAction, KeyboardDecodeMode, KeyboardDecodeTransition,
        KeyboardProtocolMode, UBOOT_BOOT_KEYBOARD_IDLE_DURATION, decode_keyboard_report_payload,
        decode_keyboard_report_payload_boot_compatible,
        decode_keyboard_report_payload_flexible_key_report, forced_keyboard_profile,
        has_report_payload, keyboard_completion_action, keyboard_decode_transition,
        keyboard_endpoint_id_matches, keyboard_payload_has_nonzero_outside_boot_window,
        keyboard_payload_prefix, keyboard_report_payload_min_len, keyboard_usage_code_valid,
        led, modifier, scancode, scancode_to_ascii, HidType, HID_LED_CONTROL_WAIT_SPINS,
        HID_KEYBOARD_INTERRUPT_READ_QUEUE_DEPTH, interrupt_read_queue_depth,
    };
    use crate::ring::completion;

    #[test]
    fn accepts_short_packet_when_report_bytes_are_complete() {
        assert!(has_report_payload(completion::SHORT_PACKET, 64, 56, 8));
    }

    #[test]
    fn rejects_short_packet_without_enough_bytes() {
        assert!(!has_report_payload(completion::SHORT_PACKET, 8, 8, 8));
        assert!(!has_report_payload(completion::SHORT_PACKET, 8, 7, 8));
    }

    #[test]
    fn rejects_non_success_completion_codes() {
        assert!(!has_report_payload(
            completion::USB_TRANSACTION_ERROR,
            8,
            0,
            8
        ));
    }

    #[test]
    fn keyboard_completion_action_keeps_polling_after_idle_or_undecoded_events() {
        assert_eq!(
            keyboard_completion_action(completion::SUCCESS, true, true),
            KeyboardCompletionAction::ReturnReport
        );
        assert_eq!(
            keyboard_completion_action(completion::SUCCESS, true, false),
            KeyboardCompletionAction::ContinuePolling
        );
        assert_eq!(
            keyboard_completion_action(completion::SHORT_PACKET, false, false),
            KeyboardCompletionAction::ContinuePolling
        );
        assert_eq!(
            keyboard_completion_action(completion::USB_TRANSACTION_ERROR, true, false),
            KeyboardCompletionAction::Error
        );
    }

    #[test]
    fn keyboard_endpoint_match_accepts_only_interrupt_in_dci() {
        assert!(keyboard_endpoint_id_matches(1, 3));
        assert!(!keyboard_endpoint_id_matches(1, 2));
        assert!(!keyboard_endpoint_id_matches(1, 1));
        assert!(!keyboard_endpoint_id_matches(1, 4));
    }

    #[test]
    fn keyboard_led_bits_match_boot_output_report_layout() {
        assert_eq!(led::NUM_LOCK, 0x01);
        assert_eq!(led::CAPS_LOCK, 0x02);
        assert_eq!(led::SCROLL_LOCK, 0x04);
        assert_eq!(
            led::NUM_LOCK | led::CAPS_LOCK | led::SCROLL_LOCK,
            0x07
        );
        assert!(HID_LED_CONTROL_WAIT_SPINS < 20_000_000);
    }

    #[test]
    fn decode_keyboard_report_payload_reads_boot_layout() {
        let payload = [0x02u8, 0x00, 0x04, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload(&payload, 0).expect("decode");
        assert_eq!(report.modifiers, 0x02);
        assert_eq!(report.keys[0], 0x04);
    }

    #[test]
    fn decode_keyboard_report_payload_reads_report_id_prefixed_layout() {
        let payload = [0x01u8, 0x02, 0x00, 0x04, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload(&payload, 1).expect("decode");
        assert_eq!(report.modifiers, 0x02);
        assert_eq!(report.keys[0], 0x04);
    }

    #[test]
    fn decode_keyboard_report_payload_reads_compact_layout() {
        let payload = [0x02u8, 0x04, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload(&payload, 0).expect("decode");
        assert_eq!(report.modifiers, 0x02);
        assert_eq!(report.keys[0], 0x04);
    }

    #[test]
    fn decode_keyboard_report_payload_reads_bitmap_layout() {
        // Bit 4 => usage 0x04 ('a')
        let payload = [0x00u8, 0x00, 0x10, 0x00];
        let report = decode_keyboard_report_payload(&payload, 0).expect("decode");
        assert_eq!(report.modifiers, 0x00);
        assert_eq!(report.keys[0], 0x04);
    }

    #[test]
    fn decode_keyboard_report_payload_reads_report_id_prefixed_bitmap_layout() {
        // [report_id, modifiers, reserved, bitmap...], bit 5 => usage 0x05 ('b')
        let payload = [0x01u8, 0x02, 0x00, 0x20, 0x00, 0x00];
        let report = decode_keyboard_report_payload(&payload, 1).expect("decode");
        assert_eq!(report.modifiers, 0x02);
        assert_eq!(report.keys[0], 0x05);
    }

    #[test]
    fn decode_keyboard_report_payload_scans_offsets_when_preferred_fails() {
        // Preferred offset=1 would fail for this payload, but offset=0 should decode.
        let payload = [0x02u8, 0x00, 0x04, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload(&payload, 1).expect("decode");
        assert_eq!(report.keys[0], 0x04);
    }

    #[test]
    fn decode_keyboard_report_payload_rejects_short_and_invalid_buffers() {
        assert!(decode_keyboard_report_payload(&[0u8; 6], 0).is_none());

        // Invalid usage code (0x02) should be rejected by layout validation.
        let payload = [0x00u8, 0x00, 0x02, 0, 0, 0, 0, 0];
        assert!(decode_keyboard_report_payload(&payload, 0).is_none());
    }

    #[test]
    fn decode_keyboard_report_payload_boot_compatible_accepts_boot_and_report_id() {
        let boot = [0x02u8, 0x00, 0x04, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload_boot_compatible(&boot, 0).expect("decode");
        assert_eq!(report.keys[0], 0x04);

        let report_id = [0x01u8, 0x02, 0x00, 0x05, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload_boot_compatible(&report_id, 1).expect("decode");
        assert_eq!(report.keys[0], 0x05);
    }

    #[test]
    fn decode_keyboard_report_payload_boot_compatible_uses_forced_report_id_when_ambiguous() {
        let report_id = [0x02u8, 0x00, 0x00, scancode::B, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload_boot_compatible(&report_id, 1).expect("decode");
        assert_eq!(report.modifiers, 0);
        assert_eq!(report.keys[0], scancode::B);
        assert!(!report.shift());
    }

    #[test]
    fn decode_keyboard_report_payload_boot_compatible_keeps_shift_for_later_boot_key_slot() {
        let boot = [modifier::LEFT_SHIFT, 0, 0, scancode::B, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload_boot_compatible(&boot, 0).expect("decode");
        assert_eq!(report.modifiers, modifier::LEFT_SHIFT);
        assert_eq!(report.keys, [0, scancode::B, 0, 0, 0, 0]);
        assert!(report.shift());
    }

    #[test]
    fn decode_keyboard_report_payload_boot_compatible_keeps_modifier_only_boot_report() {
        let boot = [modifier::LEFT_SHIFT, 0, 0, 0, 0, 0, 0, 0, 0];
        let report = decode_keyboard_report_payload_boot_compatible(&boot, 0).expect("decode");
        assert_eq!(report.modifiers, modifier::LEFT_SHIFT);
        assert!(report.shift());
        assert_eq!(report.keys, [0; 6]);
    }

    #[test]
    fn decode_keyboard_report_payload_boot_compatible_rejects_bitmap_only_reports() {
        let bitmap = [0x00u8, 0x00, 0x10, 0x00];
        assert!(decode_keyboard_report_payload_boot_compatible(&bitmap, 0).is_none());
        assert!(decode_keyboard_report_payload(&bitmap, 0).is_some());
    }

    #[test]
    fn decode_keyboard_report_payload_boot_compatible_rejects_compact_layout() {
        let compact = [0x02u8, 0x04, 0, 0, 0, 0, 0];
        assert!(decode_keyboard_report_payload_boot_compatible(&compact, 0).is_none());
        assert!(decode_keyboard_report_payload(&compact, 0).is_some());
    }

    #[test]
    fn flexible_key_report_decoder_recovers_short_compact_payload() {
        let compact = [0x00u8, scancode::A, 0, 0, 0, 0, 0];
        assert!(decode_keyboard_report_payload_boot_compatible(&compact, 0).is_none());

        let report =
            decode_keyboard_report_payload_flexible_key_report(&compact, 0).expect("decode");
        assert_eq!(report.modifiers, 0);
        assert_eq!(report.keys[0], scancode::A);
    }

    #[test]
    fn flexible_key_report_decoder_recovers_key_after_empty_boot_prefix() {
        let bitmap_after_boot_prefix = [0x00u8, 0, 0, 0, 0, 0, 0, 0, 0x10, 0];
        let strict = decode_keyboard_report_payload_boot_compatible(&bitmap_after_boot_prefix, 0)
            .expect("strict boot decode");
        assert_eq!(strict.keys, [0; 6]);
        assert!(keyboard_payload_has_nonzero_outside_boot_window(
            &bitmap_after_boot_prefix,
            0
        ));

        let report =
            decode_keyboard_report_payload_flexible_key_report(&bitmap_after_boot_prefix, 0)
                .expect("flexible key decode");
        assert!(report.keys.iter().any(|key| *key != 0));
    }

    #[test]
    fn hid_report_prefix_packs_first_eight_bytes_for_diagnostics() {
        assert_eq!(
            keyboard_payload_prefix(&[1, 2, 3, 4, 5, 6, 7, 8, 9]),
            0x0807_0605_0403_0201
        );
    }

    #[test]
    fn keyboard_report_payload_min_len_matches_boot_and_fallback_modes() {
        assert_eq!(
            keyboard_report_payload_min_len(KeyboardDecodeMode::BootCompatible, 0),
            4
        );
        assert_eq!(
            keyboard_report_payload_min_len(KeyboardDecodeMode::BootCompatible, 1),
            4
        );
        assert_eq!(
            keyboard_report_payload_min_len(KeyboardDecodeMode::Flexible, 1),
            4
        );
    }

    #[test]
    fn forced_keyboard_profile_keeps_boot_layout_strict_by_default() {
        assert_eq!(
            forced_keyboard_profile(false),
            (
                KeyboardProtocolMode::Boot,
                KeyboardDecodeMode::BootCompatible,
                0
            )
        );
        assert_eq!(
            forced_keyboard_profile(true),
            (
                KeyboardProtocolMode::Report,
                KeyboardDecodeMode::BootCompatible,
                1,
            )
        );
    }

    #[test]
    fn boot_keyboard_idle_duration_matches_uboot_polling_config() {
        assert_eq!(UBOOT_BOOT_KEYBOARD_IDLE_DURATION, 10);
    }

    #[test]
    fn keyboard_interrupt_reads_stay_armed_across_prompt_output_stalls() {
        assert_eq!(
            interrupt_read_queue_depth(HidType::Keyboard),
            HID_KEYBOARD_INTERRUPT_READ_QUEUE_DEPTH
        );
        // Low-speed boot keyboards can only report state at the endpoint
        // interval. Cohesix polls from seL4 userland, so keep about a second of
        // interrupt reads armed to cover HDMI/serial/WiFi runtime stalls without
        // losing press/release edges before local-seat can drain them.
        assert!(HID_KEYBOARD_INTERRUPT_READ_QUEUE_DEPTH >= 128);
        assert_eq!(interrupt_read_queue_depth(HidType::Mouse), 1);
        assert_eq!(interrupt_read_queue_depth(HidType::Other), 1);
    }

    #[test]
    fn keyboard_decode_transition_is_bounded_and_protocol_aware() {
        assert_eq!(
            keyboard_decode_transition(
                KeyboardProtocolMode::Boot,
                KeyboardDecodeMode::BootCompatible,
            ),
            KeyboardDecodeTransition::SwitchToReportProtocol
        );
        assert_eq!(
            keyboard_decode_transition(
                KeyboardProtocolMode::Report,
                KeyboardDecodeMode::BootCompatible,
            ),
            KeyboardDecodeTransition::RelaxDecode
        );
        assert_eq!(
            keyboard_decode_transition(KeyboardProtocolMode::Report, KeyboardDecodeMode::Flexible,),
            KeyboardDecodeTransition::None
        );
    }

    #[test]
    fn keyboard_usage_code_validation_accepts_keyboard_page_range() {
        assert!(keyboard_usage_code_valid(0));
        assert!(keyboard_usage_code_valid(0x04));
        assert!(keyboard_usage_code_valid(0xE7));
        assert!(!keyboard_usage_code_valid(0x03));
        assert!(!keyboard_usage_code_valid(0xE8));
    }

    #[test]
    fn scancode_to_ascii_uses_usb_hid_keyboard_usage_ids() {
        assert_eq!(scancode_to_ascii(scancode::A, false), Some('a'));
        assert_eq!(scancode_to_ascii(scancode::A, true), Some('A'));
        assert_eq!(scancode_to_ascii(scancode::Z, false), Some('z'));
        assert_eq!(scancode_to_ascii(scancode::N1, false), Some('1'));
        assert_eq!(scancode_to_ascii(scancode::N1, true), Some('!'));
        assert_eq!(scancode_to_ascii(scancode::ENTER, false), Some('\n'));
        assert_eq!(scancode_to_ascii(scancode::SPACE, false), Some(' '));
        assert_eq!(scancode_to_ascii(scancode::SLASH, true), Some('?'));
    }
}
