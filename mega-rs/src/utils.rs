use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::cipher::generic_array::GenericArray;
use cipher::{BlockDecrypt, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use pbkdf2::{Algorithm, Params, Pbkdf2};
use pbkdf2::password_hash::{PasswordHasher, Salt};
use rand::distributions::{Alphanumeric, DistString};
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Represents storage quotas from MEGA.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageQuotas {
    /// The amount of memory used (in bytes).
    pub memory_used: u64,
    /// The total amount of memory, used or unused (in bytes).
    pub memory_total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FileAttributes {
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "c", skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
}

impl FileAttributes {
    pub(crate) fn decrypt_and_unpack(file_key: &[u8], buffer: &mut [u8]) -> Result<Self, Error> {
        let mut cbc = cbc::Decryptor::<Aes128>::new(file_key.into(), &<_>::default());
        for chunk in buffer.chunks_exact_mut(16) {
            cbc.decrypt_block_mut(chunk.into());
        }

        assert_eq!(&buffer[..4], b"MEGA");

        let len = buffer.iter().take_while(|it| **it != b'\0').count();
        let attrs = json::from_slice(&buffer[4..len])?;

        Ok(attrs)
    }

    pub(crate) fn pack_and_encrypt(&self, file_key: &[u8]) -> Result<Vec<u8>, Error> {
        let mut buffer = b"MEGA".to_vec();
        json::to_writer(&mut buffer, self)?;

        let padding_len = (16 - buffer.len() % 16).min(15);
        buffer.extend(std::iter::repeat(b'\0').take(padding_len));

        let mut cbc = cbc::Encryptor::<Aes128>::new(file_key.into(), &<_>::default());
        for chunk in buffer.chunks_exact_mut(16) {
            cbc.encrypt_block_mut(chunk.into());
        }

        Ok(buffer)
    }
}

pub(crate) fn prepare_key_v1(password: &[u8]) -> [u8; 16] {
    let mut data = GenericArray::from([
        0x93u8, 0xC4, 0x67, 0xE3, 0x7D, 0xB0, 0xC7, 0xA4, 0xD1, 0xBE, 0x3F, 0x81, 0x01, 0x52, 0xCB,
        0x56,
    ]);

    for _ in 0..65536 {
        for chunk in password.chunks(16) {
            let mut key = [0u8; 16];
            key[0..chunk.len()].copy_from_slice(chunk);
            let aes = Aes128::new(&GenericArray::from(key));
            aes.encrypt_block(&mut data);
        }
    }

    data.into()
}

pub(crate) fn prepare_key_v2(password: &[u8], salt: &str) -> Result<Vec<u8>, Error> {
    let salt = Salt::new(salt)?;
    let params = Params {
        rounds: 100_000,
        output_length: 32,
    };

    let output = Pbkdf2.hash_password_customized(
        password,
        Some(Algorithm::Pbkdf2Sha512.ident()),
        None,
        params,
        salt,
    )?;

    let output = output.hash.unwrap();
    Ok(output.as_bytes().to_vec())
}

pub(crate) fn get_mpi(data: &[u8]) -> (rsa::BigUint, &[u8]) {
    let len = (data[0] as usize * 256 + data[1] as usize + 7) >> 3;
    let (head, tail) = data[2..].split_at(len);
    (rsa::BigUint::from_bytes_be(head), tail)
}

pub(crate) fn get_rsa_key(data: &[u8]) -> (rsa::BigUint, rsa::BigUint, rsa::BigUint) {
    let (p, data) = get_mpi(data);
    let (q, data) = get_mpi(data);
    let (d, _) = get_mpi(data);
    (p, q, d)
}

pub(crate) fn decrypt_rsa(
    m: rsa::BigUint,
    p: rsa::BigUint,
    q: rsa::BigUint,
    d: rsa::BigUint,
) -> rsa::BigUint {
    let n = p * q;
    m.modpow(&d, &n)
}

pub(crate) fn encrypt_ebc_in_place(key: &[u8], data: &mut [u8]) {
    let aes = Aes128::new(key.into());
    for block in data.chunks_mut(16) {
        aes.encrypt_block(block.into())
    }
}

pub(crate) fn decrypt_ebc_in_place(key: &[u8], data: &mut [u8]) {
    let aes = Aes128::new(key.into());
    for block in data.chunks_mut(16) {
        aes.decrypt_block(block.into())
    }
}

pub(crate) fn unmerge_key_mac(key: &mut [u8]) {
    let (fst, snd) = key.split_at_mut(16);
    for (a, b) in fst.iter_mut().zip(snd) {
        *a ^= *b;
    }
}

pub(crate) fn merge_key_mac(key: &mut [u8]) {
    let (fst, snd) = key.split_at_mut(16);
    for (a, b) in fst.iter_mut().zip(snd) {
        *a ^= *b;
    }
}

pub(crate) fn random_string(len: usize) -> String {
    let mut rng = rand::thread_rng();
    Alphanumeric.sample_string(&mut rng, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_buffer(size: usize, start: usize, step: usize) -> Vec<u8> {
        (0..size)
            .map(|i| ((start + i * step) % 255) as u8)
            .collect()
    }

    #[test]
    fn prepare_key_v1_8_bytes_test() {
        let buffer = test_buffer(8, 0, 1);
        let result = prepare_key_v1(buffer.as_slice());
        let result = hex::encode(result);

        assert_eq!(result.as_str(), "c4589a459956887caf0b408635c3c03b");
    }

    #[test]
    fn prepare_key_v1_10_bytes_test() {
        let buffer = test_buffer(10, 0, 1);
        let result = prepare_key_v1(buffer.as_slice());
        let result = hex::encode(result);

        assert_eq!(result.as_str(), "59930b1c55d783ac77df4c4ff261b0f1");
    }

    #[test]
    fn prepare_key_v1_64_bytes_test() {
        let buffer = test_buffer(64, 0, 1);
        let result = prepare_key_v1(buffer.as_slice());
        let result = hex::encode(result);

        assert_eq!(result.as_str(), "83bd84689f057f9ed9834b3ecb81d80e");
    }
}
