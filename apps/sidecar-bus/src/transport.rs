// Author: Lukas Bower
// Purpose: Own one bounded native TCP or serial field-bus exchange with fixed endpoint identity and a single whole-operation deadline.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![allow(missing_docs)]

use crate::{
    live::{BusError, Result},
    protocol,
};
use cohesix_authority::bus::{Endpoint, Operation, Parity, Protocol, Transport};
use fs2::FileExt;
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::termios::{self, BaudRate, ControlFlags, SetArg, SpecialCharacterIndices, Termios};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    os::{
        fd::{AsFd, BorrowedFd},
        unix::fs::{FileTypeExt, OpenOptionsExt},
    },
    time::{Duration, Instant},
};

pub struct Connection {
    io: Native,
    deadline: Instant,
    serial_state: Option<Termios>,
}
enum Native {
    Tcp(TcpStream),
    Serial(File),
}

impl AsFd for Native {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match self {
            Self::Tcp(stream) => stream.as_fd(),
            Self::Serial(file) => file.as_fd(),
        }
    }
}
impl Read for Native {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(bytes),
            Self::Serial(file) => file.read(bytes),
        }
    }
}
impl Write for Native {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(bytes),
            Self::Serial(file) => file.write(bytes),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Serial(file) => file.flush(),
        }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(original) = &self.serial_state {
            let _ = termios::tcsetattr(&self.io, SetArg::TCSANOW, original);
        }
    }
}

impl Connection {
    pub fn open(endpoint: &Endpoint) -> Result<Self> {
        endpoint.validate().map_err(|_| BusError::Policy)?;
        let deadline = Instant::now() + Duration::from_millis(u64::from(endpoint.timeout_ms));
        let (io, serial_state) = match &endpoint.transport {
            Transport::Tcp { address } => {
                let address: SocketAddr = address.parse().map_err(|_| BusError::Policy)?;
                let stream = TcpStream::connect_timeout(
                    &address,
                    Duration::from_millis(u64::from(endpoint.timeout_ms)),
                )
                .map_err(BusError::io)?;
                stream.set_nonblocking(true).map_err(BusError::io)?;
                stream.set_nodelay(true).map_err(BusError::io)?;
                (Native::Tcp(stream), None)
            }
            Transport::Serial { path, baud, parity } => {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(
                        (nix::fcntl::OFlag::O_NOCTTY
                            | nix::fcntl::OFlag::O_NONBLOCK
                            | nix::fcntl::OFlag::O_NOFOLLOW)
                            .bits(),
                    )
                    .open(path)
                    .map_err(BusError::io)?;
                if !file
                    .metadata()
                    .map_err(BusError::io)?
                    .file_type()
                    .is_char_device()
                {
                    return Err(BusError::Policy);
                }
                file.try_lock_exclusive().map_err(|_| BusError::Busy)?;
                let original = termios::tcgetattr(&file).map_err(|_| BusError::Transport)?;
                let mut raw = original.clone();
                termios::cfmakeraw(&mut raw);
                raw.control_flags
                    .insert(ControlFlags::CLOCAL | ControlFlags::CREAD);
                raw.control_flags.remove(
                    ControlFlags::CSIZE
                        | ControlFlags::PARENB
                        | ControlFlags::PARODD
                        | ControlFlags::CSTOPB,
                );
                raw.control_flags.insert(ControlFlags::CS8);
                match parity {
                    Parity::None if endpoint.protocol == Protocol::Modbus => {
                        raw.control_flags.insert(ControlFlags::CSTOPB)
                    }
                    Parity::None => {} // MODBUS requires 11-bit characters.
                    Parity::Even => raw.control_flags.insert(ControlFlags::PARENB),
                    Parity::Odd => raw
                        .control_flags
                        .insert(ControlFlags::PARENB | ControlFlags::PARODD),
                }
                raw.control_chars[SpecialCharacterIndices::VMIN as usize] = 0;
                raw.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
                let baud = match baud {
                    9600 => BaudRate::B9600,
                    19200 => BaudRate::B19200,
                    38400 => BaudRate::B38400,
                    57600 => BaudRate::B57600,
                    115200 => BaudRate::B115200,
                    _ => return Err(BusError::Policy),
                };
                termios::cfsetspeed(&mut raw, baud).map_err(|_| BusError::Transport)?;
                termios::tcsetattr(&file, SetArg::TCSANOW, &raw)
                    .map_err(|_| BusError::Transport)?;
                let mut connection = Self {
                    io: Native::Serial(file),
                    deadline,
                    serial_state: Some(original),
                };
                termios::tcflush(&connection.io, termios::FlushArg::TCIFLUSH)
                    .map_err(|_| BusError::Transport)?;
                connection.silent_interval(endpoint)?;
                return Ok(connection);
            }
        };
        Ok(Self {
            io,
            deadline,
            serial_state,
        })
    }

    fn silent_interval(&mut self, endpoint: &Endpoint) -> Result<()> {
        if let Transport::Serial { baud, .. } = endpoint.transport {
            let gap = Duration::from_micros(if baud > 19200 {
                1750
            } else {
                38_500_000_u64.div_ceil(u64::from(baud))
            });
            if Instant::now() + gap >= self.deadline {
                return Err(BusError::Timeout);
            }
            // The RTU inter-frame idle time is a protocol requirement, bounded
            // by the selected baud rate and included in the operation deadline.
            std::thread::sleep(gap);
        }
        Ok(())
    }

    fn ready(&self, flags: PollFlags) -> Result<()> {
        loop {
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .ok_or(BusError::Timeout)?;
            let timeout = u16::try_from(remaining.as_millis().clamp(1, 5000))
                .map_err(|_| BusError::Timeout)?;
            let mut fds = [PollFd::new(self.io.as_fd(), flags)];
            match poll(&mut fds, timeout) {
                Ok(0) => return Err(BusError::Timeout),
                Ok(_) => {
                    let events = fds[0].revents().ok_or(BusError::Transport)?;
                    if events.intersects(flags) {
                        return Ok(());
                    }
                    return Err(BusError::Disconnected);
                }
                Err(nix::errno::Errno::EINTR) => continue,
                Err(_) => return Err(BusError::Transport),
            }
        }
    }

    pub fn send(&mut self, mut bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() || bytes.len() > 292 {
            return Err(BusError::Limit);
        }
        while !bytes.is_empty() {
            self.ready(PollFlags::POLLOUT)?;
            match self.io.write(bytes) {
                Ok(0) => return Err(BusError::Disconnected),
                Ok(count) => bytes = &bytes[count..],
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(BusError::io(error)),
            }
        }
        Ok(())
    }

    fn exact(&mut self, count: usize) -> Result<Vec<u8>> {
        if count > 292 {
            return Err(BusError::Limit);
        }
        let mut bytes = vec![0; count];
        let mut cursor = 0;
        while cursor < count {
            self.ready(PollFlags::POLLIN)?;
            match self.io.read(&mut bytes[cursor..]) {
                Ok(0) => return Err(BusError::Disconnected),
                Ok(n) => cursor += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(BusError::io(error)),
            }
        }
        Ok(bytes)
    }

    pub fn receive(&mut self, endpoint: &Endpoint) -> Result<Vec<u8>> {
        match endpoint.protocol {
            Protocol::Modbus if matches!(endpoint.transport, Transport::Tcp { .. }) => {
                let mut header = self.exact(7)?;
                let length = usize::from(u16::from_be_bytes([header[4], header[5]]));
                if !(2..=254).contains(&length) {
                    return Err(BusError::Frame);
                }
                header.extend(self.exact(length - 1)?);
                Ok(header)
            }
            Protocol::Modbus => {
                let mut frame = self.exact(3)?;
                let length = if frame[1] & 0x80 != 0 {
                    5
                } else if matches!(frame[1], 5 | 6) {
                    8
                } else if matches!(frame[1], 1..=4) {
                    usize::from(frame[2]) + 5
                } else {
                    return Err(BusError::Unsupported);
                };
                if length > 256 {
                    return Err(BusError::Limit);
                }
                frame.extend(self.exact(length - 3)?);
                Ok(frame)
            }
            Protocol::Dnp3 => {
                let mut frame = self.exact(10)?;
                if frame[..2] != [5, 0x64]
                    || frame[2] < 6
                    || frame[8..10] != protocol::dnp3_crc(&frame[..8]).to_le_bytes()
                {
                    return Err(BusError::Frame);
                }
                let length = usize::from(frame[2] - 5);
                frame.extend(self.exact(length + 2 * length.div_ceil(16))?);
                Ok(frame)
            }
        }
    }
}

/// Native wire bytes are retained for exact ACK reconstruction; no unit exit code substitutes for them.
pub struct Exchange {
    pub requests: Vec<Vec<u8>>,
    pub responses: Vec<Vec<u8>>,
    pub values: Vec<protocol::Value>,
}

pub fn exchange(endpoint: &Endpoint, operation: &Operation, sequence: u16) -> Result<Exchange> {
    let mut connection = Connection::open(endpoint)?;
    let mut result = Exchange {
        requests: Vec::new(),
        responses: Vec::new(),
        values: Vec::new(),
    };
    match endpoint.protocol {
        Protocol::Modbus => {
            let tcp = matches!(endpoint.transport, Transport::Tcp { .. });
            let request = protocol::modbus_request(endpoint.unit, sequence, operation, tcp)?;
            connection.send(&request)?;
            let response = connection.receive(endpoint)?;
            result.values =
                protocol::modbus_response(endpoint.unit, sequence, operation, tcp, &response)?;
            result.requests.push(request);
            result.responses.push(response);
        }
        Protocol::Dnp3 => {
            let phases: &[bool] = if operation.is_control() {
                &[false, true]
            } else {
                &[false]
            };
            let mut transport_sequence = sequence as u8;
            for (offset, operate) in phases.iter().enumerate() {
                let seq = (sequence as u8).wrapping_add(offset as u8);
                let request = protocol::dnp3_request_sequences(
                    endpoint,
                    operation,
                    seq,
                    transport_sequence,
                    *operate,
                )?;
                connection.send(&request)?;
                transport_sequence = transport_sequence.wrapping_add(1);
                let response = connection.receive(endpoint)?;
                let (values, confirm) =
                    protocol::dnp3_response(endpoint, operation, seq, *operate, &response)?;
                result.requests.push(request);
                result.responses.push(response);
                if confirm {
                    let confirmation = protocol::dnp3_frame(
                        endpoint.master,
                        endpoint.outstation,
                        &[0xc0 | (transport_sequence & 0x3f), 0xc0 | (seq & 0xf), 0],
                    )?;
                    connection.send(&confirmation)?;
                    result.requests.push(confirmation);
                    transport_sequence = transport_sequence.wrapping_add(1);
                }
                result.values = values;
            }
        }
    }
    Ok(result)
}
