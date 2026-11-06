//! Convergence integration tests for replicated trust store and device registry.

use cctv_core::{CameraConfig, CameraType};
use cctv_pair::trust::TrustedPeer;
use cctv_sync::{SyncDeviceRegistry, SyncTrustStore};

#[test]
fn test_sync_trust_store_bidirectional_convergence() {
    let mut node_a = SyncTrustStore::new("node-A");
    let mut node_b = SyncTrustStore::new("node-B");

    let peer1 = TrustedPeer {
        device_id: "cam-front-door".to_string(),
        name: "Front Door".to_string(),
        psk: None,
        paired_at_ms: 1000,
    };

    let peer2 = TrustedPeer {
        device_id: "cam-backyard".to_string(),
        name: "Backyard".to_string(),
        psk: None,
        paired_at_ms: 1100,
    };

    // Node A pairs cam 1
    node_a.insert_peer(peer1.clone(), 1000);
    // Node B pairs cam 2
    node_b.insert_peer(peer2.clone(), 1100);

    // Merge A into B and B into A
    node_b.merge(&node_a);
    node_a.merge(&node_b);

    assert!(node_a.contains("cam-front-door"));
    assert!(node_a.contains("cam-backyard"));
    assert!(node_b.contains("cam-front-door"));
    assert!(node_b.contains("cam-backyard"));

    assert_eq!(node_a.list_peers().len(), 2);
    assert_eq!(node_b.list_peers().len(), 2);

    // Node B revokes cam 1
    node_b.revoke_peer("cam-front-door");
    assert!(!node_b.contains("cam-front-door"));

    // Sync revocation back to Node A
    node_a.merge(&node_b);
    assert!(!node_a.contains("cam-front-door"));
    assert_eq!(node_a.list_peers().len(), 1);
}

#[test]
fn test_sync_device_registry_convergence_and_tie_breaking() {
    let mut reg_a = SyncDeviceRegistry::new("node-A");
    let mut reg_b = SyncDeviceRegistry::new("node-B");

    let mut cfg1 = CameraConfig::new("cam-ptz-1", "onvif", "192.168.1.50");
    cfg1.label = Some("Living Room PTZ".into());
    cfg1.ty = CameraType::Ptz;

    reg_a.register_camera(cfg1.clone(), 1000);

    // Node B concurrently registers same camera with updated host & label at higher timestamp
    let mut cfg2 = cfg1.clone();
    cfg2.host = "192.168.1.150".into();
    cfg2.label = Some("Living Room PTZ HD".into());
    reg_b.register_camera(cfg2, 1200);

    // Converge
    reg_a.merge(&reg_b);
    reg_b.merge(&reg_a);

    let fetched_a = reg_a.get("cam-ptz-1").expect("camera present");
    let fetched_b = reg_b.get("cam-ptz-1").expect("camera present");

    assert_eq!(fetched_a.host, "192.168.1.150");
    assert_eq!(fetched_b.host, "192.168.1.150");
    assert_eq!(fetched_a.label.as_deref(), Some("Living Room PTZ HD"));
    assert_eq!(fetched_b.label.as_deref(), Some("Living Room PTZ HD"));
}
