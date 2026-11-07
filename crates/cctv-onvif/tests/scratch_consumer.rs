//! Test backward-compatibility of the cctv-onvif re-export shim.

#![allow(deprecated)]

use cctv_core::CameraConfig;
use cctv_onvif::*;

#[test]
fn scratch_consumer_compiles_and_uses_shim() {
    let cfg = CameraConfig::new("cam-1", "onvif", "192.168.1.10");
    assert_eq!(cfg.driver, "onvif");

    let creds = Credentials {
        username: "admin".to_string(),
        password: "password".to_string(),
    };
    assert_eq!(creds.username, "admin");

    let discovery = OnvifDiscovery::new();
    let _ = discovery;
}
