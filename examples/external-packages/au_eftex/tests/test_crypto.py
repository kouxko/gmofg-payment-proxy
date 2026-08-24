import unittest

from au_eftex.crypto import (
    derive_data_request_key,
    derive_data_response_key,
    derive_ipek,
    derive_mac_request_key,
    derive_mac_response_key,
    derive_pin_key,
    derive_transaction_key,
    expand_tdes_key,
    tdes_ofb_decrypt,
    tdes_ofb_encrypt,
)


# Public ANSI X9.24-1 / legacy 2-key TDES DUKPT example values.
BDK = bytes.fromhex("0123456789ABCDEFFEDCBA9876543210")
KSN = bytes.fromhex("FFFF9876543210E00008")
IPEK = bytes.fromhex("6AC292FAA1315B4D858AB3A3D7D5933A")
TRANSACTION_KEY = bytes.fromhex("27F66D5244FF62E1AA6F6120EDEB4280")


class DukptTests(unittest.TestCase):
    def test_derives_public_ansi_ipek_vector(self) -> None:
        self.assertEqual(derive_ipek(BDK, KSN), IPEK)

    def test_derives_public_ansi_transaction_key_vector(self) -> None:
        self.assertEqual(derive_transaction_key(IPEK, KSN), TRANSACTION_KEY)

    def test_rejects_reserved_zero_transaction_counter(self) -> None:
        with self.assertRaisesRegex(ValueError, "transaction counter"):
            derive_transaction_key(IPEK, bytes.fromhex("FFFF9876543210E00000"))

    def test_derives_all_usage_variants(self) -> None:
        self.assertEqual(
            derive_pin_key(TRANSACTION_KEY),
            bytes.fromhex("27F66D5244FF621EAA6F6120EDEB427F"),
        )
        self.assertEqual(
            derive_mac_request_key(TRANSACTION_KEY),
            bytes.fromhex("27F66D5244FF9DE1AA6F6120EDEBBD80"),
        )
        self.assertEqual(
            derive_mac_response_key(TRANSACTION_KEY),
            bytes.fromhex("27F66D52BBFF62E1AA6F612012EB4280"),
        )
        self.assertEqual(
            derive_data_request_key(TRANSACTION_KEY),
            bytes.fromhex("C39B2778B058AC376FB18DC906F75CBA"),
        )

        self.assertEqual(
            derive_data_response_key(TRANSACTION_KEY),
            bytes.fromhex("846E267CB822197406DA2B161191C6E4"),
        )


class TripleDesTests(unittest.TestCase):
    def test_expands_two_key_tdes_as_k1_k2_k1(self) -> None:
        key = bytes.fromhex("00112233445566778899AABBCCDDEEFF")
        self.assertEqual(expand_tdes_key(key), key + key[:8])

    def test_ofb_round_trip_preserves_non_block_aligned_data(self) -> None:
        key = bytes.fromhex("0123456789ABCDEFFEDCBA9876543210")
        iv = bytes.fromhex("1020304050607080")
        plaintext = b"synthetic-au-eftex-payload"

        ciphertext = tdes_ofb_encrypt(key, iv, plaintext)

        self.assertNotEqual(ciphertext, plaintext)
        self.assertEqual(tdes_ofb_decrypt(key, iv, ciphertext), plaintext)

    def test_ofb_matches_an_openssl_golden_vector(self) -> None:
        key = bytes.fromhex("C39B2778B058AC376FB18DC906F75CBA")
        iv = bytes.fromhex("0102030405060708")
        plaintext = bytes.fromhex(
            "3132303020000000000000013030303030300102030405060708FFFFFFFFFF05"
        )
        expected = bytes.fromhex(
            "7E19FF3B90D3CDA958C159CC43F66B19982A737CAC9E93E9F0435EC21D54A5F8"
        )

        self.assertEqual(tdes_ofb_encrypt(key, iv, plaintext), expected)
        self.assertEqual(tdes_ofb_decrypt(key, iv, expected), plaintext)

    def test_rejects_invalid_key_and_iv_lengths(self) -> None:
        with self.assertRaisesRegex(ValueError, "16 bytes"):
            expand_tdes_key(b"short")
        with self.assertRaisesRegex(ValueError, "8 bytes"):
            tdes_ofb_encrypt(BDK, b"short", b"data")


if __name__ == "__main__":
    unittest.main()
