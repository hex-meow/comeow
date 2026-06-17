//! A minimal mock CANopen node for testing the client without hardware.
//!
//! Answers expedited SDO uploads for a couple of objects, acknowledges
//! any download, and emits one EMCY frame at startup. Run it on a vcan:
//!
//! ```sh
//! sudo modprobe vcan && sudo ip link add dev vcan0 type vcan && sudo ip link set up vcan0
//! cargo run --example mock_node -- vcan0 5
//! ```

use std::sync::Arc;

use can_transport::socketcan::SocketCanBus;
use can_transport::{CanBus, CanFilter, CanFrame, CanId};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let iface = std::env::args().nth(1).unwrap_or_else(|| "vcan0".into());
    let node: u8 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(5);

    let bus = Arc::new(SocketCanBus::open(&iface)?);
    let rsdo = 0x600 + node as u16; // client -> server
    let tsdo = 0x580 + node as u16; // server -> client
    let emcy = 0x080 + node as u16;

    // Emit an EMCY every second: 0x3120 (voltage), error register 0x04.
    {
        let bus = bus.clone();
        tokio::spawn(async move {
            loop {
                let _ = bus
                    .send(CanFrame::new_data(emcy, &[0x20, 0x31, 0x04, 0xAA, 0xBB, 0, 0, 0]).unwrap())
                    .await;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }
    println!("mock node {node} on {iface}: emitting EMCY, serving SDO at 0x{rsdo:03X}");

    let mut rx = bus.subscribe(CanFilter::exact_standard(rsdo)).await?;
    loop {
        let frame = rx.recv().await?;
        let d = frame.data();
        if d.len() < 8 {
            continue;
        }
        let ccs = d[0] >> 5;
        let index = u16::from_le_bytes([d[1], d[2]]);
        let sub = d[3];
        let resp = match ccs {
            2 => upload_response(index, sub), // initiate upload (read)
            1 => Some([0x60, d[1], d[2], d[3], 0, 0, 0, 0]), // initiate download (write) ack
            _ => None,
        };
        if let Some(payload) = resp {
            bus.send(CanFrame::new_data(CanId::Standard(tsdo), &payload)?)
                .await?;
            println!("  <- req idx 0x{index:04X}:{sub:02X} ccs {ccs}, replied");
        }
    }
}

fn upload_response(index: u16, sub: u8) -> Option<[u8; 8]> {
    let (len, val): (u8, u32) = match (index, sub) {
        (0x1018, 0x00) => (1, 4),       // highest sub-index = 4
        (0x1017, 0x00) => (2, 1000),    // heartbeat time u16
        (0x1000, 0x00) => (4, 0x00020192), // device type u32
        _ => return None,
    };
    let n = 4 - len; // bytes not containing data
    let cmd = 0x43 | (n << 2); // expedited, size indicated
    let b = val.to_le_bytes();
    let i = index.to_le_bytes();
    Some([cmd, i[0], i[1], sub, b[0], b[1], b[2], b[3]])
}
