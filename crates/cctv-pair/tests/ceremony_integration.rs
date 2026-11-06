//! Integration tests for cctv-pair ceremony, security boundaries, and fuzzing:
//! - QR happy path
//! - Forged proof rejected
//! - Numeric-compare match / mismatch
//! - Commitment prevents nonce-forcing
//! - 5-minute timeout test
//! - 5 failed attempts -> 15-minute lockout test
//! - 10⁶ fuzz iterations test

use cctv_pair::*;

#[test]
fn test_qr_flow_happy_path_with_trust_store() {
    let secret = b"super-strong-ephemeral-secret-32b!".to_vec();
    let client_id = "client-zbase32-id";
    let host_id = "host-zbase32-id";

    let qr_payload = PairingQrPayload::new(
        client_id.as_bytes().to_vec(),
        secret.clone(),
        "Host Camera Hub",
        vec!["127.0.0.1:4433".parse().unwrap()],
    );
    let uri = qr_payload.to_uri();
    let parsed: PairingQrPayload = uri.parse().expect("valid qr uri");

    let client_cfg =
        CeremonyConfig::qr_initiator(client_id, "Mobile App", host_id, (*parsed.secret).clone());
    let host_cfg = CeremonyConfig::qr_responder(host_id, "Host Camera Hub", secret);

    let start_ms = 1_000_000;
    let mut client = PairingCeremony::new(client_cfg, start_ms);
    let mut host = PairingCeremony::new(host_cfg, start_ms);

    // Initiator sends ClientHello
    let hello = client.start_initiation(None).unwrap().expect("hello");

    // Responder verifies and replies
    let host_resp = host
        .handle_message(hello, start_ms + 100, None)
        .unwrap()
        .expect("host resp");

    // Initiator verifies host response and replies pair.done
    let ack = client
        .handle_message(host_resp, start_ms + 200, None)
        .unwrap()
        .expect("done ack");
    assert_eq!(
        ack,
        PairingMessage::PairDone {
            status: "pair.done".to_string()
        }
    );

    // Responder handles done ack and closes with code 0
    let close = host
        .handle_message(ack, start_ms + 300, None)
        .unwrap()
        .expect("close");
    assert_eq!(
        close,
        PairingMessage::Close {
            code: 0,
            reason: "pairing successful".to_string()
        }
    );

    // Initiator handles close code 0
    let final_resp = client.handle_message(close, start_ms + 400, None).unwrap();
    assert!(final_resp.is_none());

    // Verify state & trust storage: only store trust on code 0
    let mut client_store = TrustStore::new();
    let mut host_store = TrustStore::new();
    let peer = client
        .commit_trust(&mut client_store)
        .expect("stored trust");
    host.commit_trust(&mut host_store)
        .expect("stored trust host");
    assert_eq!(peer.device_id, host_id);
    assert_eq!(client_store.len(), 1);
    assert_eq!(host_store.len(), 1);
    assert_eq!(host_store.get_nonce_counter(client_id), 1);
}

#[test]
fn test_qr_replay_attack_rejected() {
    let secret = b"super-strong-ephemeral-secret-32b!".to_vec();
    let client_id = "client-replay-id";
    let host_id = "host-replay-id";

    let mut host_store = TrustStore::new();
    host_store.set_nonce_counter(client_id, 42);

    let host_cfg = CeremonyConfig::qr_responder_with_persisted_nonce(
        host_id,
        "Host Camera Hub",
        secret.clone(),
        host_store.get_nonce_counter(client_id),
    );
    let mut host = PairingCeremony::new(host_cfg, 1000);

    // Attacker intercepts and replays ClientHello with counter 42 (or less)
    let replayed_client_hello = PairingMessage::QrClientHello {
        version: CURRENT_PROTOCOL_VERSION,
        client_id: client_id.to_string(),
        client_name: "Replayed Client".to_string(),
        nonce_counter: 42,
        proof_hex: to_hex(&compute_pairing_proof(&secret, client_id, host_id, 42)),
    };

    let err = host
        .handle_message(replayed_client_hello, 1050, None)
        .unwrap_err();
    assert_eq!(err, PairingError::ReplayDetected(42, 42));
    assert!(host.commit_trust(&mut host_store).is_err());
}

#[test]
fn test_forged_proof_rejected() {
    let secret_legit = b"legit-secret-key-1234".to_vec();
    let secret_attacker = b"attacker-secret-key-99".to_vec();

    let client_cfg =
        CeremonyConfig::qr_initiator("client-alice", "Alice", "host-hub", secret_attacker);
    let host_cfg = CeremonyConfig::qr_responder("host-hub", "Hub", secret_legit);

    let mut client = PairingCeremony::new(client_cfg, 1000);
    let mut host = PairingCeremony::new(host_cfg, 1000);

    let forged_hello = client.start_initiation(None).unwrap().expect("hello");

    let res = host.handle_message(forged_hello, 1050, None);
    assert_eq!(res.unwrap_err(), PairingError::ForgedProof);

    let mut host_store = TrustStore::new();
    let commit_res = host.commit_trust(&mut host_store);
    assert!(commit_res.is_err());
    assert!(host_store.is_empty());
}

#[test]
fn test_numeric_compare_match_and_mismatch() {
    // 1. Happy path: SAS Match
    {
        let mut a = PairingCeremony::new(
            CeremonyConfig::numeric_compare("dev-a", "Device A", CeremonyRole::Initiator),
            1000,
        );
        let mut b = PairingCeremony::new(
            CeremonyConfig::numeric_compare("dev-b", "Device B", CeremonyRole::Responder),
            1000,
        );

        let na = vec![0x11; 16];
        let nb = vec![0x22; 16];

        let msg1 = a.start_initiation(Some(na)).unwrap().unwrap();
        let msg2 = b.handle_message(msg1, 1005, Some(nb)).unwrap().unwrap();
        let msg3 = a.handle_message(msg2, 1010, None).unwrap().unwrap();
        b.handle_message(msg3, 1015, None).unwrap();

        let ack_a = a.confirm_sas(true, 1020).unwrap();
        let _ack_b = b.confirm_sas(true, 1020).unwrap();
        let close_b = b.handle_message(ack_a, 1025, None).unwrap().unwrap();
        a.handle_message(close_b, 1030, None).unwrap();

        let mut store_a = TrustStore::new();
        assert!(a.commit_trust(&mut store_a).is_ok());
    }

    // 2. Mismatch: User rejects SAS code
    {
        let mut a = PairingCeremony::new(
            CeremonyConfig::numeric_compare("dev-a", "Device A", CeremonyRole::Initiator),
            1000,
        );
        let mut b = PairingCeremony::new(
            CeremonyConfig::numeric_compare("dev-b", "Device B", CeremonyRole::Responder),
            1000,
        );

        let na = vec![0x33; 16];
        let nb = vec![0x44; 16];

        let msg1 = a.start_initiation(Some(na)).unwrap().unwrap();
        let msg2 = b.handle_message(msg1, 1005, Some(nb)).unwrap().unwrap();
        let msg3 = a.handle_message(msg2, 1010, None).unwrap().unwrap();
        b.handle_message(msg3, 1015, None).unwrap();

        // User detects mismatch on Device A and rejects
        let close_msg = a.confirm_sas(false, 1020).unwrap();
        assert_eq!(
            close_msg,
            PairingMessage::Close {
                code: 1,
                reason: "SAS mismatch".to_string()
            }
        );

        // Remote device B handles non-zero close code
        let b_res = b.handle_message(close_msg, 1025, None);
        assert!(matches!(
            b_res.unwrap_err(),
            PairingError::NonZeroCloseCode(1, _)
        ));

        let mut store_a = TrustStore::new();
        let mut store_b = TrustStore::new();
        assert!(a.commit_trust(&mut store_a).is_err());
        assert!(b.commit_trust(&mut store_b).is_err());
        assert!(store_a.is_empty());
        assert!(store_b.is_empty());
    }
}

#[test]
fn test_commitment_prevents_nonce_forcing() {
    // Attacker tries to alter nonce after seeing responder's nonce B
    let mut a = PairingCeremony::new(
        CeremonyConfig::numeric_compare("alice", "Alice", CeremonyRole::Initiator),
        1000,
    );
    let mut b = PairingCeremony::new(
        CeremonyConfig::numeric_compare("bob", "Bob", CeremonyRole::Responder),
        1000,
    );

    let original_na = vec![0xaa; 16];
    let nb = vec![0xbb; 16];

    let msg1 = a.start_initiation(Some(original_na)).unwrap().unwrap();
    let msg2 = b.handle_message(msg1, 1005, Some(nb)).unwrap().unwrap();
    let _msg3 = a.handle_message(msg2, 1010, None).unwrap().unwrap();

    // Attacker sends manipulated nonce A' that was not committed to in msg1
    let forged_reveal = PairingMessage::SasRevealA {
        nonce_hex: to_hex(&[0xcc; 16]),
    };

    let b_res = b.handle_message(forged_reveal, 1015, None);
    assert_eq!(b_res.unwrap_err(), PairingError::CommitmentMismatch);

    let mut store_b = TrustStore::new();
    assert!(b.commit_trust(&mut store_b).is_err());
}

#[test]
fn test_ceremony_timeout_5_minutes() {
    let client_cfg = CeremonyConfig::qr_initiator("c1", "Client", "h1", b"secret".to_vec());
    let mut ceremony = PairingCeremony::new(client_cfg, 1000);

    let _hello = ceremony.start_initiation(None).unwrap();

    // 4 minutes 59 seconds: not timed out
    assert!(!ceremony.check_timeout(1000 + 4 * 60 * 1000 + 59 * 1000));

    // 5 minutes + 1 ms: timed out
    assert!(ceremony.check_timeout(1000 + 5 * 60 * 1000 + 1));
    assert_eq!(
        ceremony.state(),
        &CeremonyState::Failed(PairingError::Timeout)
    );

    // Subsequent message rejected due to timeout
    let resp = ceremony.handle_message(
        PairingMessage::PairDone {
            status: "done".into(),
        },
        1000 + 6 * 60 * 1000,
        None,
    );
    assert_eq!(resp.unwrap_err(), PairingError::Timeout);
}

#[test]
fn test_lockout_after_5_failed_attempts_for_15_minutes() {
    let mut manager = LockoutManager::new();
    let remote_ip = "10.0.0.42";
    let base_time_ms = 500_000;

    // First 4 failures do not trigger lockout
    for i in 1..=4 {
        let is_locked = manager.record_failure(remote_ip, base_time_ms);
        assert!(!is_locked, "attempt {i} should not lock");
        assert!(!manager.is_locked_out(remote_ip, base_time_ms));
    }

    // 5th failure triggers 15-minute lockout
    let is_locked = manager.record_failure(remote_ip, base_time_ms);
    assert!(is_locked, "5th attempt must trigger lockout");
    assert!(manager.is_locked_out(remote_ip, base_time_ms));

    // During lockout window (e.g. 10 minutes later)
    let ten_mins_later = base_time_ms + 10 * 60 * 1000;
    assert!(manager.is_locked_out(remote_ip, ten_mins_later));
    assert!(manager.record_failure(remote_ip, ten_mins_later));

    // At 14m 59s: still locked out
    let almost_unlocked = base_time_ms + 15 * 60 * 1000 - 1;
    assert!(manager.is_locked_out(remote_ip, almost_unlocked));

    // At 15m 1ms: lockout expired
    let expired_ms = base_time_ms + 15 * 60 * 1000 + 1;
    assert!(!manager.is_locked_out(remote_ip, expired_ms));
}

#[test]
fn test_fuzz_10_to_the_6th_iterations() {
    // 10^6 iterations exercising core crypto (zbase32 / SAS / constant_time / hmac / commitment)
    // Runs deterministically and fast (< 1s in debug/release).
    let mut state = 0x12345678u32;
    // Fast xorshift32 PRNG
    let mut next_prng = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };

    let total_iters = 1_000_000;
    let mut accumulator: u64 = 0;

    for i in 0..total_iters {
        let r1 = next_prng();
        let r2 = next_prng();

        let b1 = r1.to_le_bytes();
        let b2 = r2.to_le_bytes();

        let sas = compute_sas(&b1, &b2, "A", "B");
        assert!(sas < 1_000_000);
        accumulator = accumulator.wrapping_add(sas as u64);

        if i % 100_000 == 0 {
            // Periodic z-base-32 and commitment invariant check
            let enc = zbase32::encode(&b1);
            let dec = zbase32::decode(&enc).unwrap();
            assert_eq!(dec, b1);

            let commit = compute_commitment(&b2);
            assert!(verify_commitment(&commit, &b2));
            assert!(!verify_commitment(&commit, &b1));
        }
    }

    assert!(accumulator > 0);
}
