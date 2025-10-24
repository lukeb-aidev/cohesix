// Author: Lukas Bower

use core::fmt::Write;

use heapless::{Deque, Vec};
use sel4_sys::{seL4_CPtr, seL4_Error, seL4_ObjectType};
use spin::Mutex;

use super::{dec_u32, hex_u64, DebugPutc};

const TRACE_DEPTH: usize = 64;
const MAX_FIELDS: usize = 10;

/// Records maintained for bootstrap tracing.
#[derive(Clone)]
pub struct BootstrapRecord {
    sequence: u32,
    label_hash: u32,
    label: &'static str,
    fields: Vec<Field, MAX_FIELDS>,
}

impl BootstrapRecord {
    /// Creates a new record tagged with the provided label.
    pub fn new(label: &'static str) -> Self {
        Self {
            sequence: 0,
            label_hash: fnv1a32(label),
            label,
            fields: Vec::new(),
        }
    }

    /// Sets the sequence number associated with the record.
    fn set_sequence(&mut self, sequence: u32) {
        self.sequence = sequence;
    }

    /// Pushes a field onto the record, discarding it if the record is at capacity.
    pub fn push_field(&mut self, field: Field) {
        let _ = self.fields.push(field);
    }

    /// Returns the record sequence number.
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// Returns the hashed label associated with the record.
    pub fn label_hash(&self) -> u32 {
        self.label_hash
    }

    /// Returns the label associated with the record.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Returns the recorded fields.
    pub fn fields(&self) -> &[Field] {
        self.fields.as_slice()
    }
}

/// Describes a named field included with a [`BootstrapRecord`].
#[derive(Clone, Copy)]
pub struct Field {
    name: &'static str,
    value: FieldKind,
}

impl Field {
    /// Creates a field containing a hexadecimal value.
    pub const fn hex(name: &'static str, value: u64) -> Self {
        Self {
            name,
            value: FieldKind::Hex(value),
        }
    }

    /// Creates a field containing a decimal value.
    pub const fn decimal(name: &'static str, value: u32) -> Self {
        Self {
            name,
            value: FieldKind::Decimal(value),
        }
    }

    /// Creates a field storing a capability object type.
    pub const fn object_type(name: &'static str, value: seL4_ObjectType) -> Self {
        Self {
            name,
            value: FieldKind::ObjectType(value),
        }
    }

    /// Creates a field describing an seL4 error.
    pub const fn error(name: &'static str, value: seL4_Error) -> Self {
        Self {
            name,
            value: FieldKind::Error(value),
        }
    }

    /// Creates a textual field.
    pub const fn text(name: &'static str, value: &'static str) -> Self {
        Self {
            name,
            value: FieldKind::Text(value),
        }
    }

    /// Returns the name of the field.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the value stored in the field.
    pub const fn value(&self) -> FieldKind {
        self.value
    }
}

/// Enumerates the types of data tracked by a [`Field`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Hexadecimal value that must never be dereferenced as a pointer.
    Hex(u64),
    /// Decimal numeric value.
    Decimal(u32),
    /// Object type reported by seL4.
    ObjectType(seL4_ObjectType),
    /// seL4 error value.
    Error(seL4_Error),
    /// Descriptive text.
    Text(&'static str),
}

struct TraceBuffer {
    sequence: u32,
    records: Deque<BootstrapRecord, TRACE_DEPTH>,
}

impl TraceBuffer {
    const fn new() -> Self {
        Self {
            sequence: 0,
            records: Deque::new(),
        }
    }

    fn push(&mut self, mut record: BootstrapRecord) {
        record.set_sequence(self.sequence);
        self.sequence = self.sequence.wrapping_add(1);
        if self.records.is_full() {
            let _ = self.records.pop_front();
        }
        let _ = self.records.push_back(record);
    }

    fn snapshot<const N: usize>(&self, out: &mut Vec<BootstrapRecord, N>) {
        out.clear();
        for record in self.records.iter() {
            let _ = out.push(record.clone());
        }
    }

    fn clear(&mut self) {
        self.sequence = 0;
        self.records.clear();
    }
}

static TRACE: Mutex<TraceBuffer> = Mutex::new(TraceBuffer::new());

/// Computes the 32-bit FNV-1a hash of the supplied label.
fn fnv1a32(label: &str) -> u32 {
    const OFFSET: u32 = 0x811C_9DC5;
    const PRIME: u32 = 0x0100_0193;

    let mut hash = OFFSET;
    for byte in label.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Records a retype event into the bootstrap trace buffer.
pub(crate) fn record_retype_event(
    phase: &'static str,
    untyped: seL4_CPtr,
    obj_type: seL4_ObjectType,
    size_bits: u32,
    dst_cnode: seL4_CPtr,
    node_index: seL4_CPtr,
    node_depth: u8,
    node_offset: seL4_CPtr,
    num_objects: u32,
    err: Option<seL4_Error>,
) {
    let mut record = BootstrapRecord::new("retype");
    record.push_field(Field::text("phase", phase));
    record.push_field(Field::hex("ut", untyped as u64));
    record.push_field(Field::object_type("obj", obj_type));
    record.push_field(Field::decimal("size_bits", size_bits));
    record.push_field(Field::hex("root", dst_cnode as u64));
    record.push_field(Field::hex("idx", node_index as u64));
    record.push_field(Field::decimal("depth", u32::from(node_depth)));
    record.push_field(Field::hex("off", node_offset as u64));
    record.push_field(Field::decimal("num", num_objects));
    if let Some(error) = err {
        record.push_field(Field::error("err", error));
    }

    TRACE.lock().push(record);
}

/// Writes the recorded events to the UART console.
pub(crate) fn flush_to_uart() {
    let mut writer = DebugPutc;
    let guard = TRACE.lock();
    for record in guard.records.iter() {
        let _ = write!(
            writer,
            "[boot:{} seq={} hash=0x{:08x}",
            record.label(),
            record.sequence(),
            record.label_hash()
        );

        for field in record.fields() {
            let _ = writer.write_str(" ");
            let _ = writer.write_str(field.name());
            let _ = writer.write_char('=');
            match field.value() {
                FieldKind::Hex(value) => {
                    hex_u64(&mut writer, value);
                }
                FieldKind::Decimal(value) => {
                    dec_u32(&mut writer, value);
                }
                FieldKind::ObjectType(value) => {
                    let _ = write!(writer, "{:?}", value);
                }
                FieldKind::Error(value) => {
                    let _ = write!(writer, "{:?}", value);
                }
                FieldKind::Text(value) => {
                    let _ = writer.write_str(value);
                }
            }
        }

        let _ = writer.write_str("]\n");
    }
}

/// Copies the currently recorded events into the provided buffer.
pub(crate) fn snapshot<const N: usize>(out: &mut Vec<BootstrapRecord, N>) {
    TRACE.lock().snapshot(out);
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    TRACE.lock().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable() {
        assert_eq!(fnv1a32("retype"), 0xd66a_de58);
        assert_eq!(fnv1a32("phase"), 0xd980_31f8);
    }

    #[test]
    fn retype_record_is_stored() {
        reset_for_tests();
        record_retype_event(
            "pre",
            0x2000,
            sel4_sys::seL4_ObjectType::seL4_EndpointObject,
            12,
            0x4000,
            0x0,
            0,
            0x20,
            1,
            None,
        );

        let mut records: Vec<BootstrapRecord, 4> = Vec::new();
        snapshot(&mut records);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.label(), "retype");
        assert_eq!(record.sequence(), 0);
        assert_eq!(record.fields()[0].name(), "phase");
        assert_eq!(record.fields()[0].value(), FieldKind::Text("pre"));
    }

    #[test]
    fn ring_buffer_drops_oldest_entry() {
        reset_for_tests();
        for index in 0..(TRACE_DEPTH as u32 + 2) {
            record_retype_event(
                "pre",
                index as seL4_CPtr,
                sel4_sys::seL4_ObjectType::seL4_EndpointObject,
                0,
                0,
                0,
                0,
                0,
                1,
                None,
            );
        }

        let mut records: Vec<BootstrapRecord, TRACE_DEPTH> = Vec::new();
        snapshot(&mut records);
        assert_eq!(records.len(), TRACE_DEPTH);
        let first = &records[0];
        assert_eq!(first.sequence(), 2);
        if let FieldKind::Hex(value) = first.fields()[1].value() {
            assert_eq!(value, 2);
        } else {
            panic!("unexpected field layout");
        }
    }
}
