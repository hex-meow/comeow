//! Background listener that decodes and pretty-prints EMCY frames from
//! any node on the bus, without disrupting the interactive prompt.

use std::sync::Arc;

use can_transport::{CanBus, CanFilter, CanId, CanIoError, FrameKind};

use crate::printer::LinePrinter;

/// Subscribe to the EMCY COB-ID range and print decoded frames via the
/// given printer until the bus disconnects.
pub fn spawn(bus: Arc<dyn CanBus>, mut printer: Box<dyn LinePrinter>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // EMCY COB-ID = 0x080 | node (node 1..127 => 0x081..=0x0FF).
        // Mask 0x780 with id 0x080 matches exactly that range (plus 0x080,
        // which is SYNC and has node 0 — filtered out below).
        let filter = CanFilter::standard(0x080, 0x780);
        let mut rx = match bus.subscribe(filter).await {
            Ok(rx) => rx,
            Err(e) => {
                log::error!("EMCY subscribe failed: {e}");
                return;
            }
        };

        loop {
            match rx.recv().await {
                Ok(frame) => {
                    if let Some(line) = decode(&frame) {
                        printer.print_line(line);
                    }
                }
                Err(CanIoError::Lagged { dropped }) => {
                    printer.print_line(format!("[EMCY] listener lagged, dropped {dropped} frames"));
                }
                Err(_) => break, // Disconnected / backend error: stop quietly.
            }
        }
    })
}

fn decode(frame: &can_transport::CanFrame) -> Option<String> {
    if !matches!(frame.kind(), FrameKind::Data) {
        return None;
    }
    let CanId::Standard(id) = frame.id() else {
        return None;
    };
    let node = (id & 0x7F) as u8;
    if node == 0 {
        return None; // 0x080 = SYNC, not an EMCY.
    }
    let d = frame.data();
    if d.len() < 8 {
        return None;
    }
    let err_code = u16::from_le_bytes([d[0], d[1]]);
    let err_reg = d[2];
    let msef = &d[3..8];
    Some(format!(
        "[EMCY] node 0x{node:02X}: code 0x{err_code:04X} ({}) | reg 0x{err_reg:02X} [{}] | vendor {:02X?}",
        emcy_class(err_code),
        err_register_flags(err_reg),
        msef,
    ))
}

/// Human label for the high byte of a CiA 301 standard error code.
fn emcy_class(code: u16) -> &'static str {
    // 0xFFxx is reserved for device-specific codes; otherwise group by the
    // high nibble (e.g. 0x31xx falls under the 0x3xxx "voltage" group).
    if code & 0xFF00 == 0xFF00 {
        return "device specific";
    }
    match code & 0xF000 {
        0x0000 => "no error / reset",
        0x1000 => "generic error",
        0x2000 => "current",
        0x3000 => "voltage",
        0x4000 => "temperature",
        0x5000 => "device hardware",
        0x6000 => "device software",
        0x7000 => "additional modules",
        0x8000 => "monitoring (comm/protocol)",
        0x9000 => "external error",
        0xF000 => "additional functions",
        _ => "other",
    }
}

/// Decode the CiA 301 error-register bitfield (object 0x1001).
fn err_register_flags(reg: u8) -> String {
    if reg == 0 {
        return "none".into();
    }
    let mut flags = Vec::new();
    if reg & 0x01 != 0 {
        flags.push("generic");
    }
    if reg & 0x02 != 0 {
        flags.push("current");
    }
    if reg & 0x04 != 0 {
        flags.push("voltage");
    }
    if reg & 0x08 != 0 {
        flags.push("temperature");
    }
    if reg & 0x10 != 0 {
        flags.push("communication");
    }
    if reg & 0x20 != 0 {
        flags.push("profile");
    }
    if reg & 0x80 != 0 {
        flags.push("manufacturer");
    }
    flags.join(",")
}
