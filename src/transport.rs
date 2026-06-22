use tokio::io::AsyncRead;

/// A connected Bluetooth transport byte stream.
///
/// Both the Linux BlueZ RFCOMM stream and the macOS serial-port file implement
/// `tokio::io::AsyncRead`, so the runtime treats them uniformly through this
/// boxed, sendable, unpin alias.
pub type TransportStream = Box<dyn AsyncRead + Unpin + Send>;

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{bail, Context, Result};
    use bluer::{
        rfcomm::{Profile, Role},
        Address, Session,
    };
    use futures::StreamExt;
    use std::time::Duration;
    use tokio::time::timeout;
    use tracing::info;
    use uuid::Uuid;

    use super::TransportStream;

    const SPP_UUID: Uuid = Uuid::from_u128(0x00001101_0000_1000_8000_00805f9b34fb);
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
    const PROFILE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

    pub fn parse_device_address(device_addr: &str) -> Result<Address> {
        device_addr
            .parse::<Address>()
            .with_context(|| format!("'{device_addr}' is not a valid Bluetooth address"))
    }

    pub async fn connect_transport(device_addr: &str) -> Result<TransportStream> {
        let session = Session::new().await.context("create BlueZ session")?;
        let adapter = session
            .default_adapter()
            .await
            .context("get default Bluetooth adapter")?;
        adapter
            .set_powered(true)
            .await
            .context("power Bluetooth adapter")?;

        let device_addr = parse_device_address(device_addr)?;
        let device = adapter
            .device(device_addr)
            .context("get BlueZ device handle")?;

        let profile = Profile {
            uuid: SPP_UUID,
            role: Some(Role::Client),
            auto_connect: Some(false),
            require_authentication: Some(false),
            require_authorization: Some(false),
            name: Some("ptt-audio-menu SPP client".to_string()),
            ..Default::default()
        };

        let mut profile_handle = session
            .register_profile(profile)
            .await
            .context("register RFCOMM Serial Port profile")?;

        info!(profile_uuid = %SPP_UUID, "registered RFCOMM profile");
        info!("connecting RFCOMM profile");
        let mut connect_task = tokio::spawn({
            let device = device.clone();
            async move { device.connect_profile(&SPP_UUID).await }
        });

        info!("waiting for RFCOMM profile connection");
        let request = timeout(PROFILE_REQUEST_TIMEOUT, async {
            tokio::select! {
                request = profile_handle.next() => {
                    request.context("profile connection stream ended before NewConnection")
                }
                connect_result = &mut connect_task => {
                    match connect_result {
                        Ok(Ok(())) => bail!("BlueZ profile connection returned before NewConnection"),
                        Ok(Err(err)) => Err(err).context("request BlueZ profile connection"),
                        Err(err) => Err(err).context("join BlueZ profile connection task"),
                    }
                }
            }
        })
        .await
        .context("timed out waiting for RFCOMM profile NewConnection")?
        .context("wait for RFCOMM profile NewConnection")?;

        info!(
            device = %request.device(),
            version = ?request.version(),
            features = ?request.features(),
            "accepted RFCOMM profile connection"
        );
        let stream = request
            .accept()
            .context("accept RFCOMM profile connection")?;

        timeout(CONNECT_TIMEOUT, connect_task)
            .await
            .context("timed out completing BlueZ profile connection")?
            .context("join BlueZ profile connection task")?
            .context("complete BlueZ profile connection")?;

        Ok(Box::new(stream))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use anyhow::{Context, Result};
    use tokio_serial::SerialStream;
    use tracing::info;

    use super::TransportStream;

    /// macOS does not provide a MAC-address-keyed lookup for the SPP serial
    /// device node; the path is derived from the device's Bluetooth name and
    /// must be supplied via `bluetooth.serial_port` in the config.
    pub async fn connect_transport(serial_port: &str) -> Result<TransportStream> {
        info!(serial_port, "opening Bluetooth SPP serial port");
        let builder = tokio_serial::new(serial_port, 9600);
        let stream = SerialStream::open(&builder)
            .with_context(|| format!("open serial port {serial_port}"))?;
        Ok(Box::new(stream) as TransportStream)
    }
}

#[cfg(target_os = "linux")]
pub use linux::connect_transport;
#[cfg(target_os = "macos")]
pub use macos::connect_transport;
