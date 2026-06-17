//! Backend selection. All `#[cfg(feature)]` backend code is isolated here.

use std::sync::Arc;

use anyhow::{bail, Context};
use can_transport::CanBus;

use crate::cli::Cli;

/// Open the CAN bus selected on the command line, returning an
/// `Arc<dyn CanBus>` that can be shared across the executor and EMCY tasks.
pub async fn open_bus(cli: &Cli) -> anyhow::Result<Arc<dyn CanBus>> {
    if cli.gs_usb {
        if cli.iface.is_some() {
            bail!("cannot use a SocketCAN interface and --gs-usb together");
        }
        return open_gs_usb(cli).await;
    }
    // Default backend: SocketCAN. Interface defaults to `can0`.
    let iface = cli.iface.as_deref().unwrap_or("can0");
    open_socketcan(iface)
}

#[cfg(feature = "socketcan")]
fn open_socketcan(iface: &str) -> anyhow::Result<Arc<dyn CanBus>> {
    use can_transport::socketcan::SocketCanBus;
    let bus = SocketCanBus::open(iface)
        .with_context(|| format!("failed to open SocketCAN interface `{iface}`"))?;
    Ok(Arc::new(bus))
}

#[cfg(not(feature = "socketcan"))]
fn open_socketcan(_iface: &str) -> anyhow::Result<Arc<dyn CanBus>> {
    bail!("socketcan backend not compiled in; rebuild with --features socketcan");
}

#[cfg(feature = "gs_usb")]
async fn open_gs_usb(cli: &Cli) -> anyhow::Result<Arc<dyn CanBus>> {
    use can_transport::gs_usb::{GsUsbBus, GsUsbConfig};
    let cfg = if cli.fd {
        GsUsbConfig::fd_1m_5m()
    } else {
        GsUsbConfig::classic_1m()
    }
    .with_channel(cli.channel);
    let bus = GsUsbBus::open(cfg)
        .await
        .context("failed to open gs_usb adapter")?;
    Ok(Arc::new(bus))
}

#[cfg(not(feature = "gs_usb"))]
async fn open_gs_usb(_cli: &Cli) -> anyhow::Result<Arc<dyn CanBus>> {
    bail!("gs_usb backend not compiled in; rebuild with --features gs_usb");
}
