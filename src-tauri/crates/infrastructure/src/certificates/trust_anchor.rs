use base64::{Engine as _, engine::general_purpose::STANDARD};

pub(super) fn certificate_chain_to_pem(certificates: &[Vec<u8>]) -> Vec<u8> {
    let mut pem = Vec::new();
    for certificate in certificates {
        pem.extend_from_slice(b"-----BEGIN CERTIFICATE-----\n");
        let encoded = STANDARD.encode(certificate);
        for line in encoded.as_bytes().chunks(64) {
            pem.extend_from_slice(line);
            pem.push(b'\n');
        }
        pem.extend_from_slice(b"-----END CERTIFICATE-----\n");
    }
    pem
}
