//! Cryptographic primitives for pairing ceremony:
//! - HMAC-SHA256
//! - SHA256 commitments
//! - Constant-time comparison
//! - SAS (Short Authentication String) numeric comparison calculation

use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// Block size of SHA-256 in bytes.
const SHA256_BLOCK_SIZE: usize = 64;

/// Computes HMAC-SHA256 of `msg` using the specified `key`.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; SHA256_BLOCK_SIZE];
    if key.len() > SHA256_BLOCK_SIZE {
        let mut hasher = Sha256::new();
        hasher.update(key);
        let hash = hasher.finalize();
        k[..32].copy_from_slice(&hash);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; SHA256_BLOCK_SIZE];
    let mut opad = [0x5cu8; SHA256_BLOCK_SIZE];
    for i in 0..SHA256_BLOCK_SIZE {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let out = outer.finalize();

    k.zeroize();
    ipad.zeroize();
    opad.zeroize();

    out.into()
}

/// Computes the SHA-256 digest of data.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Constant-time comparison of two byte slices to prevent timing side-channels.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut res = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        res |= x ^ y;
    }
    res == 0
}

/// Computes QR / Session pairing proof:
/// `HMAC-SHA256(S, "cctv-pair-v0" ‖ client_id ‖ host_id)`
pub fn compute_pairing_proof(secret: &[u8], client_id: &str, host_id: &str) -> [u8; 32] {
    let mut msg = Vec::with_capacity(12 + client_id.len() + host_id.len());
    msg.extend_from_slice(b"cctv-pair-v0");
    msg.extend_from_slice(client_id.as_bytes());
    msg.extend_from_slice(host_id.as_bytes());
    hmac_sha256(secret, &msg)
}

/// Verifies QR / Session pairing proof in constant time.
pub fn verify_pairing_proof(
    secret: &[u8],
    client_id: &str,
    host_id: &str,
    expected_proof: &[u8; 32],
) -> bool {
    let actual_proof = compute_pairing_proof(secret, client_id, host_id);
    constant_time_eq(&actual_proof, expected_proof)
}

/// Computes commitment `c = SHA-256(nonce)`.
pub fn compute_commitment(nonce: &[u8]) -> [u8; 32] {
    sha256(nonce)
}

/// Verifies that commitment `c == SHA-256(nonce)` in constant time.
pub fn verify_commitment(commitment: &[u8; 32], nonce: &[u8]) -> bool {
    let expected = compute_commitment(nonce);
    constant_time_eq(commitment, &expected)
}

/// Computes the 6-digit numeric comparison SAS code:
/// `sas = uint32(HMAC-SHA256(nA ‖ nB, "cctv-sas-v0" ‖ idA ‖ idB)[0..4]) mod 1_000_000`
pub fn compute_sas(na: &[u8], nb: &[u8], ida: &str, idb: &str) -> u32 {
    let mut key = Vec::with_capacity(na.len() + nb.len());
    key.extend_from_slice(na);
    key.extend_from_slice(nb);

    let mut msg = Vec::with_capacity(11 + ida.len() + idb.len());
    msg.extend_from_slice(b"cctv-sas-v0");
    msg.extend_from_slice(ida.as_bytes());
    msg.extend_from_slice(idb.as_bytes());

    let h = hmac_sha256(&key, &msg);
    let raw = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
    raw % 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha256_rfc4231_case1() {
        // Test case 1 from RFC 4231
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let expected = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        let result = hmac_sha256(&key, data);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_pairing_proof_and_verify() {
        let secret = b"super-secret-key-123";
        let client_id = "client-dev-1";
        let host_id = "host-hub-9";

        let proof = compute_pairing_proof(secret, client_id, host_id);
        assert!(verify_pairing_proof(secret, client_id, host_id, &proof));

        let mut bad_proof = proof;
        bad_proof[0] ^= 1;
        assert!(!verify_pairing_proof(
            secret, client_id, host_id, &bad_proof
        ));
        assert!(!verify_pairing_proof(
            b"wrong-secret",
            client_id,
            host_id,
            &proof
        ));
    }

    #[test]
    fn test_sas_computation() {
        let na = [1u8; 16];
        let nb = [2u8; 16];
        let sas1 = compute_sas(&na, &nb, "node-a", "node-b");
        assert!(sas1 < 1_000_000);
        let sas2 = compute_sas(&na, &nb, "node-a", "node-b");
        assert_eq!(sas1, sas2);

        // Different nonce yields different SAS
        let nb_diff = [3u8; 16];
        let sas3 = compute_sas(&na, &nb_diff, "node-a", "node-b");
        assert_ne!(sas1, sas3);
    }

    #[test]
    fn test_commitment() {
        let nonce = b"random-nonce-bytes-12345678";
        let c = compute_commitment(nonce);
        assert!(verify_commitment(&c, nonce));
        assert!(!verify_commitment(&c, b"different-nonce"));
    }
}
