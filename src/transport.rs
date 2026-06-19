use anyhow::{bail, Context, Result};
use bluer::{
    rfcomm::{Profile, Role, Stream},
    Address, Session,
};
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

const SPP_UUID: Uuid = Uuid::from_u128(0x00001101_0000_1000_8000_00805f9b34fb);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PROFILE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn connect_rfcomm_stream(device_addr: &str) -> Result<Stream> {
    let session = Session::new().await.context("create BlueZ session")?;
    let adapter = session
        .default_adapter()
        .await
        .context("get default Bluetooth adapter")?;
    adapter
        .set_powered(true)
        .await
        .context("power Bluetooth adapter")?;

    let device_addr: Address = device_addr.parse().context("parse device address")?;
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

    println!("profile uuid={SPP_UUID}");
    println!("connecting profile");
    let mut connect_task = tokio::spawn({
        let device = device.clone();
        async move { device.connect_profile(&SPP_UUID).await }
    });

    println!("waiting for RFCOMM profile connection");
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

    println!(
        "accepted device={} version={:?} features={:?}",
        request.device(),
        request.version(),
        request.features()
    );
    let stream = request
        .accept()
        .context("accept RFCOMM profile connection")?;

    timeout(CONNECT_TIMEOUT, connect_task)
        .await
        .context("timed out completing BlueZ profile connection")?
        .context("join BlueZ profile connection task")?
        .context("complete BlueZ profile connection")?;

    Ok(stream)
}
