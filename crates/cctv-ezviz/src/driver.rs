//! The Ezviz driver — account-level operations.
//!
//! Where the Ezviz protocol meets the cctv device model.  Every public method
//! speaks device-model terms; the Ezviz vocabulary stops at this boundary.  The
//! per-camera half lives in [`crate::EzvizCameraBackend`].

use ezviz_openapi::alarm::{AlarmItem, AlarmStatus, AlarmType};
use ezviz_openapi::device::{CameraChannel, DeviceInfo};
use ezviz_openapi::EzvizClient;

use crate::error::Result;
use crate::types::{Alarm, Connectivity, EzvizCamera};

/// How many devices one discovery sweep asks for (the platform caps a page).
const DISCOVER_PAGE: i32 = 50;

/// The Ezviz driver, bound to one account.
pub struct Driver {
    client: EzvizClient,
}

impl Driver {
    /// Build from a configured Ezviz Open API client.
    pub fn new(client: EzvizClient) -> Self {
        Self { client }
    }

    /// The underlying Ezviz Open API client.
    pub fn client(&self) -> &EzvizClient {
        &self.client
    }

    /// List the account's cameras, translated into the device model.
    ///
    /// Devices and channels are fetched once each and joined on the device
    /// serial; a device the platform reports no channel for still yields one
    /// camera, because on Ezviz the device itself is the camera.
    pub async fn discover(&self) -> Result<Vec<EzvizCamera>> {
        let devices = self.client.list_devices(0, DISCOVER_PAGE).await?.items;
        let channels = self.client.list_cameras().await?;

        let mut cameras = Vec::with_capacity(devices.len());
        for device in &devices {
            let mut matched = false;
            for channel in channels
                .iter()
                .filter(|c| c.device_serial == device.device_serial)
            {
                cameras.push(camera_from_channel(device, channel));
                matched = true;
            }
            if !matched {
                cameras.push(camera_from_device(device));
            }
        }
        Ok(cameras)
    }

    /// Recent alarms, reduced to the device model.
    ///
    /// `since_ms` is a Unix-**millisecond** lower bound (the platform only
    /// searches the last seven days); `limit` caps the page.
    pub async fn alarms(&self, since_ms: Option<i64>, limit: i32) -> Result<Vec<Alarm>> {
        let page = self
            .client
            .list_alarms(since_ms, None, AlarmType::All, AlarmStatus::All, 0, limit)
            .await?;
        Ok(page.items.into_iter().map(alarm_from_item).collect())
    }

    /// Ask a camera to take a still.
    ///
    /// The platform delivers the picture out of band, through its picture
    /// service — this only issues the request, so nothing is returned.
    pub async fn capture(&self, device_serial: &str, channel: i32) -> Result<()> {
        self.client
            .capture(device_serial, channel)
            .await
            .map_err(Into::into)
    }
}

/// One camera built from a device the platform reports no channel for.
fn camera_from_device(device: &DeviceInfo) -> EzvizCamera {
    EzvizCamera {
        id: device.device_serial.clone(),
        label: device.device_name.clone(),
        ip: None,
        mac: None,
        firmware: device.device_version.clone(),
        stream_url: None,
        rtmp_url: None,
        ptz: false,
        ai: false,
        connectivity: Connectivity::Cloud,
    }
}

/// One camera per channel, channel-qualified so a multi-channel device cannot
/// collide with itself.
fn camera_from_channel(device: &DeviceInfo, channel: &CameraChannel) -> EzvizCamera {
    let serial = channel
        .ipc_serial
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(channel.device_serial.as_str());
    EzvizCamera {
        id: format!("{serial}:{}", channel.channel_no),
        label: channel
            .channel_name
            .clone()
            .or_else(|| channel.device_name.clone())
            .unwrap_or_else(|| device.device_name.clone()),
        ip: None,
        mac: None,
        firmware: device.device_version.clone(),
        stream_url: None,
        rtmp_url: None,
        ptz: false,
        ai: false,
        connectivity: Connectivity::Cloud,
    }
}

/// One alarm, reduced to the device model.
fn alarm_from_item(item: AlarmItem) -> Alarm {
    Alarm {
        id: item.alarm_id,
        device_serial: item.device_serial.unwrap_or_default(),
        channel: item.channel_no.unwrap_or(1),
        kind: item.alarm_type,
        at_ms: item.alarm_time,
        read: item.is_checked == Some(1),
        snapshot_url: item.alarm_pic_url,
    }
}
