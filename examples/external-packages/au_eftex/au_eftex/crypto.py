"""Traditional two-key TDES DUKPT primitives used by the AU EFTEX package.

The functions in this module are deliberately side-effect free. They accept and
return bytes, and never log or persist keys or message contents.
"""

from Crypto.Cipher import DES, DES3


_KEY_REGISTER_MASK = bytes.fromhex("C0C0C0C000000000C0C0C0C000000000")
_PIN_VARIANT_MASK = bytes.fromhex("00000000000000FF00000000000000FF")
_MAC_REQUEST_VARIANT_MASK = bytes.fromhex("000000000000FF00000000000000FF00")
_MAC_RESPONSE_VARIANT_MASK = bytes.fromhex("00000000FF00000000000000FF000000")
_DATA_REQUEST_VARIANT_MASK = bytes.fromhex("0000000000FF00000000000000FF0000")
_DATA_RESPONSE_VARIANT_MASK = bytes.fromhex("000000FF00000000000000FF00000000")
_KSN_COUNTER_MASK = 0x1FFFFF
_KSN_WITHOUT_COUNTER_MASK = ((1 << 80) - 1) ^ _KSN_COUNTER_MASK


def _require_length(name: str, value: bytes, length: int) -> None:
    if not isinstance(value, bytes):
        raise TypeError(f"{name} must be bytes")
    if len(value) != length:
        raise ValueError(f"{name} must be {length} bytes")


def _xor(left: bytes, right: bytes) -> bytes:
    return bytes(a ^ b for a, b in zip(left, right))


def expand_tdes_key(key: bytes) -> bytes:
    """Expand a 16-byte two-key TDES key from K1-K2 to K1-K2-K1."""

    _require_length("TDES key", key, 16)
    return key + key[:8]


def _tdes_ecb_encrypt(key: bytes, block: bytes) -> bytes:
    _require_length("TDES block", block, 8)
    return DES3.new(expand_tdes_key(key), DES3.MODE_ECB).encrypt(block)


def derive_ipek(bdk: bytes, ksn: bytes) -> bytes:
    """Derive an Initial PIN Encryption Key from a BDK and 10-byte KSN."""

    _require_length("BDK", bdk, 16)
    _require_length("KSN", ksn, 10)
    masked_ksn = (int.from_bytes(ksn, "big") & _KSN_WITHOUT_COUNTER_MASK).to_bytes(
        10, "big"
    )
    register = masked_ksn[:8]
    return _tdes_ecb_encrypt(bdk, register) + _tdes_ecb_encrypt(
        _xor(bdk, _KEY_REGISTER_MASK), register
    )


def _non_reversible_key_generation(key: bytes, ksn_register: int) -> bytes:
    register = (ksn_register & ((1 << 64) - 1)).to_bytes(8, "big")

    def encrypt_register(register_key: bytes) -> bytes:
        left, right = register_key[:8], register_key[8:]
        return _xor(DES.new(left, DES.MODE_ECB).encrypt(_xor(register, right)), right)

    return encrypt_register(_xor(key, _KEY_REGISTER_MASK)) + encrypt_register(key)


def derive_transaction_key(ipek: bytes, ksn: bytes) -> bytes:
    """Derive the transaction key for the KSN's 21-bit transaction counter."""

    _require_length("IPEK", ipek, 16)
    _require_length("KSN", ksn, 10)
    ksn_value = int.from_bytes(ksn, "big")
    counter = ksn_value & _KSN_COUNTER_MASK
    if counter == 0:
        raise ValueError("KSN transaction counter must be non-zero")

    register = ksn_value & _KSN_WITHOUT_COUNTER_MASK
    key = ipek
    bit = 1 << 20
    while bit:
        if counter & bit:
            register |= bit
            key = _non_reversible_key_generation(key, register)
        bit >>= 1
    return key


def _derive_variant(transaction_key: bytes, mask: bytes) -> bytes:
    _require_length("transaction key", transaction_key, 16)
    return _xor(transaction_key, mask)


def derive_pin_key(transaction_key: bytes) -> bytes:
    """Derive the PIN-encryption usage variant."""

    return _derive_variant(transaction_key, _PIN_VARIANT_MASK)


def derive_mac_request_key(transaction_key: bytes) -> bytes:
    """Derive the request-MAC usage variant."""

    return _derive_variant(transaction_key, _MAC_REQUEST_VARIANT_MASK)


def derive_mac_response_key(transaction_key: bytes) -> bytes:
    """Derive the response-MAC usage variant."""

    return _derive_variant(transaction_key, _MAC_RESPONSE_VARIANT_MASK)


def _derive_data_key(transaction_key: bytes, mask: bytes) -> bytes:
    variant = _derive_variant(transaction_key, mask)
    cipher = DES3.new(expand_tdes_key(variant), DES3.MODE_ECB)
    return cipher.encrypt(variant[:8]) + cipher.encrypt(variant[8:])


def derive_data_request_key(transaction_key: bytes) -> bytes:
    """Derive and one-way-transform the request-data usage variant."""

    return _derive_data_key(transaction_key, _DATA_REQUEST_VARIANT_MASK)


def derive_data_response_key(transaction_key: bytes) -> bytes:
    """Derive and one-way-transform the response-data usage variant."""

    return _derive_data_key(transaction_key, _DATA_RESPONSE_VARIANT_MASK)


def tdes_ofb_encrypt(key: bytes, iv: bytes, plaintext: bytes) -> bytes:
    """Encrypt arbitrary-length data using two-key TDES in OFB mode."""

    _require_length("TDES key", key, 16)
    _require_length("TDES OFB IV", iv, 8)
    if not isinstance(plaintext, bytes):
        raise TypeError("plaintext must be bytes")
    return DES3.new(expand_tdes_key(key), DES3.MODE_OFB, iv=iv).encrypt(plaintext)


def tdes_ofb_decrypt(key: bytes, iv: bytes, ciphertext: bytes) -> bytes:
    """Decrypt arbitrary-length data using two-key TDES in OFB mode."""

    _require_length("TDES key", key, 16)
    _require_length("TDES OFB IV", iv, 8)
    if not isinstance(ciphertext, bytes):
        raise TypeError("ciphertext must be bytes")
    return DES3.new(expand_tdes_key(key), DES3.MODE_OFB, iv=iv).decrypt(ciphertext)
