use cipher::{Block, BlockCipherEncrypt, KeyInit};
use des::{Des, TdesEde3};

const KEY_REGISTER_MASK: [u8; 16] = hex("C0C0C0C000000000C0C0C0C000000000");
const DATA_REQUEST_MASK: [u8; 16] = hex("0000000000FF00000000000000FF0000");
const DATA_RESPONSE_MASK: [u8; 16] = hex("000000FF00000000000000FF00000000");
const COUNTER_MASK: u128 = 0x1f_ffff;
const KSN_80_MASK: u128 = (1u128 << 80) - 1;

const fn hex(value: &str) -> [u8; 16] {
    let bytes = value.as_bytes();
    let mut output = [0; 16];
    let mut index = 0;
    while index < 16 {
        output[index] = (nibble(bytes[index * 2]) << 4) | nibble(bytes[index * 2 + 1]);
        index += 1;
    }
    output
}

const fn nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn xor<const N: usize>(left: [u8; N], right: [u8; N]) -> [u8; N] {
    let mut output = [0; N];
    for index in 0..N {
        output[index] = left[index] ^ right[index];
    }
    output
}

fn tdes_encrypt(key: [u8; 16], block: [u8; 8]) -> [u8; 8] {
    let mut expanded = [0; 24];
    expanded[..16].copy_from_slice(&key);
    expanded[16..].copy_from_slice(&key[..8]);
    let cipher = TdesEde3::new_from_slice(&expanded).expect("fixed TDES key length");
    let mut block = Block::<TdesEde3>::from(block);
    cipher.encrypt_block(&mut block);
    block.into()
}

fn des_encrypt(key: [u8; 8], block: [u8; 8]) -> [u8; 8] {
    let cipher = Des::new_from_slice(&key).expect("fixed DES key length");
    let mut block = Block::<Des>::from(block);
    cipher.encrypt_block(&mut block);
    block.into()
}

pub fn derive_ipek(bdk: [u8; 16], ksn: [u8; 10]) -> [u8; 16] {
    let value = u128::from_be_bytes(pad_ksn(ksn)) & (KSN_80_MASK ^ COUNTER_MASK);
    let register: [u8; 8] = value.to_be_bytes()[6..14].try_into().unwrap();
    let mut output = [0; 16];
    output[..8].copy_from_slice(&tdes_encrypt(bdk, register));
    output[8..].copy_from_slice(&tdes_encrypt(xor(bdk, KEY_REGISTER_MASK), register));
    output
}

pub fn derive_transaction_key(ipek: [u8; 16], ksn: [u8; 10]) -> Result<[u8; 16], String> {
    let ksn_value = u128::from_be_bytes(pad_ksn(ksn));
    let counter = ksn_value & COUNTER_MASK;
    if counter == 0 {
        return Err("KSN transaction counter must be non-zero".into());
    }
    let mut register = ksn_value & (KSN_80_MASK ^ COUNTER_MASK);
    let mut key = ipek;
    let mut bit = 1u128 << 20;
    while bit != 0 {
        if counter & bit != 0 {
            register |= bit;
            key = non_reversible(key, register);
        }
        bit >>= 1;
    }
    Ok(key)
}

fn non_reversible(key: [u8; 16], register: u128) -> [u8; 16] {
    let block: [u8; 8] = register.to_be_bytes()[8..].try_into().unwrap();
    let generate = |key: [u8; 16]| {
        let left: [u8; 8] = key[..8].try_into().unwrap();
        let right: [u8; 8] = key[8..].try_into().unwrap();
        xor(des_encrypt(left, xor(block, right)), right)
    };
    let mut output = [0; 16];
    output[..8].copy_from_slice(&generate(xor(key, KEY_REGISTER_MASK)));
    output[8..].copy_from_slice(&generate(key));
    output
}

pub fn derive_data_key(transaction_key: [u8; 16], upstream: bool) -> [u8; 16] {
    let variant = xor(
        transaction_key,
        if upstream {
            DATA_REQUEST_MASK
        } else {
            DATA_RESPONSE_MASK
        },
    );
    let mut output = [0; 16];
    output[..8].copy_from_slice(&tdes_encrypt(variant, variant[..8].try_into().unwrap()));
    output[8..].copy_from_slice(&tdes_encrypt(variant, variant[8..].try_into().unwrap()));
    output
}

pub fn ofb(key: [u8; 16], iv: [u8; 8], input: &[u8]) -> Vec<u8> {
    let mut feedback = iv;
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(8) {
        feedback = tdes_encrypt(key, feedback);
        output.extend(chunk.iter().zip(feedback).map(|(byte, mask)| byte ^ mask));
    }
    output
}

fn pad_ksn(ksn: [u8; 10]) -> [u8; 16] {
    let mut output = [0; 16];
    output[6..].copy_from_slice(&ksn);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_public_ansi_dukpt_vectors() {
        let bdk = hex("0123456789ABCDEFFEDCBA9876543210");
        let ksn: [u8; 10] = [0xff, 0xff, 0x98, 0x76, 0x54, 0x32, 0x10, 0xe0, 0x00, 0x08];
        let ipek = derive_ipek(bdk, ksn);
        assert_eq!(ipek, hex("6AC292FAA1315B4D858AB3A3D7D5933A"));
        let transaction = derive_transaction_key(ipek, ksn).unwrap();
        assert_eq!(transaction, hex("27F66D5244FF62E1AA6F6120EDEB4280"));
        assert_eq!(
            derive_data_key(transaction, true),
            hex("C39B2778B058AC376FB18DC906F75CBA")
        );
        assert_eq!(
            derive_data_key(transaction, false),
            hex("846E267CB822197406DA2B161191C6E4")
        );
    }
}
