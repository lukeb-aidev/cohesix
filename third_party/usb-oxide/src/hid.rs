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
    ring::PhysMem,
};

use alloc::sync::Arc;
use core::{hint::spin_loop, ptr};

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
    report_buf: PhysMem<H>,
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

        // Allocate report buffer (64-byte alignment for DMA)
        let host = device.ctrl().host();
        let report_buf = PhysMem::alloc(host, ep_in.max_packet_size as usize, 64)?;

        let hid = Self {
            device,
            hid_type,
            interface: iface.interface_number,
            ep_in: ep_in.number(),
            ep_max_packet: ep_in.max_packet_size,
            report_buf,
        };

        // Set boot protocol for boot devices
        if iface.interface_subclass == hid_subclass::BOOT {
            hid.set_protocol(0)?; // Boot protocol
        }

        // Set idle rate to 0 (only report on change)
        let _ = hid.set_idle(0, 0);

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

        let setup = SetupPacket::hid_set_report(self.interface, report_type::OUTPUT, 0, 1);
        let mut buf = [leds];
        self.device.control_transfer(&setup, Some(&mut buf))?;
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
        self.device.queue_transfer(
            self.ep_in,
            true,
            &self.report_buf,
            self.ep_max_packet as usize,
        )
    }

    /// Poll for keyboard report (non-blocking) with transfer re-queue errors surfaced.
    pub fn poll_keyboard_checked(&self) -> Result<Option<KeyboardReport>> {
        if self.hid_type != HidType::Keyboard {
            return Ok(None);
        }

        if let Some(evt) = self.device.ctrl().poll_event()
            && evt.slot_id() == self.device.slot_id()
        {
            let code = evt.completion_code();
            if code == 1 || code == 13 {
                // SUCCESS or SHORT_PACKET
                let report = unsafe { *(self.report_buf.as_ptr::<KeyboardReport>()) };

                // Re-queue for next report
                self.queue_read()?;

                return Ok(Some(report));
            }
        }
        Ok(None)
    }

    /// Poll for keyboard report (non-blocking).
    pub fn poll_keyboard(&self) -> Option<KeyboardReport> {
        self.poll_keyboard_checked().ok().flatten()
    }

    /// Poll for mouse report (non-blocking)
    pub fn poll_mouse(&self) -> Option<MouseReport> {
        if self.hid_type != HidType::Mouse {
            return None;
        }

        if let Some(evt) = self.device.ctrl().poll_event()
            && evt.slot_id() == self.device.slot_id()
        {
            let code = evt.completion_code();
            if code == 1 || code == 13 {
                let report = unsafe { *(self.report_buf.as_ptr::<MouseReport>()) };

                // Re-queue for next report
                let _ = self.queue_read();

                return Some(report);
            }
        }
        None
    }

    /// Blocking read for keyboard
    pub fn read_keyboard(&self) -> Result<KeyboardReport> {
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
    pub fn read_mouse(&self) -> Result<MouseReport> {
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
        // Note: report_buf will be freed when PhysMem is dropped
        // but we need to explicitly free it since PhysMem doesn't auto-free
        unsafe {
            host.free(
                self.report_buf.virt(),
                self.report_buf.size(),
                self.report_buf.align(),
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
