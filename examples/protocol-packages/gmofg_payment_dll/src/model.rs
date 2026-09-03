use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DllDocument {
    #[serde(rename = "ErrorCode")]
    pub(crate) error_code: String,
    #[serde(rename = "ResponseID")]
    pub(crate) response_id: String,
    #[serde(rename = "TransactionType")]
    pub(crate) transaction_type: String,
    #[serde(rename = "ResponseTimeStamp")]
    pub(crate) response_timestamp: String,
    #[serde(rename = "DebitInp", skip_serializing_if = "Option::is_none")]
    pub(crate) debit_input: Option<String>,
    #[serde(rename = "TerminalID", skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_id: Option<String>,
    #[serde(rename = "TerminalSeqNo", skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_sequence_number: Option<String>,
    #[serde(rename = "ResponseDateTime", skip_serializing_if = "Option::is_none")]
    pub(crate) response_date_time: Option<String>,
    #[serde(rename = "DLLUpdateAnn", skip_serializing_if = "Option::is_none")]
    pub(crate) dll_update_announcement: Option<String>,
    #[serde(rename = "KCCI_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_card_companies: Option<Vec<CardCompany>>,
    #[serde(rename = "KJSI_01", skip_serializing_if = "Option::is_none")]
    pub(crate) merchant: Option<MerchantInfo>,
    #[serde(rename = "KBAT_01", skip_serializing_if = "Option::is_none")]
    pub(crate) batch: Option<BatchInfo>,
    #[serde(rename = "KDST_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_terminal_ap: Option<TerminalApInfo>,
    #[serde(rename = "KCAK_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_ca_public_keys: Option<Vec<CaPublicKeyGroup>>,
    #[serde(rename = "KBRD_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_brands: Option<Vec<BrandInfo>>,
    #[serde(rename = "KICC_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_ic_card_companies: Option<Vec<IcCardCompany>>,
    #[serde(rename = "KGIN_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay: Option<UnionPayInfo>,
    #[serde(rename = "GDST_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_terminal_ap: Option<TerminalApInfo>,
    #[serde(rename = "GCAK_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_ca_public_keys: Option<Vec<CaPublicKeyGroup>>,
    #[serde(rename = "GBRD_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_brands: Option<Vec<BrandInfo>>,
    #[serde(rename = "GICC_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_ic_card_companies: Option<Vec<IcCardCompany>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CardCompany {
    pub(crate) kid: String,
    pub(crate) acquirer_code: String,
    pub(crate) card_issuer_name: String,
    pub(crate) manual_input: String,
    pub(crate) input_selection_info: String,
    pub(crate) password_input: String,
    pub(crate) payment_type_selection: String,
    pub(crate) card_ranges: Vec<CardRange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CardRange {
    pub(crate) from: String,
    pub(crate) to: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MerchantInfo {
    pub(crate) phone_number_1: String,
    pub(crate) phone_number_2: String,
    #[serde(rename = "merchant_info_1")]
    pub(crate) info_1: String,
    #[serde(rename = "merchant_info_2")]
    pub(crate) info_2: String,
    pub(crate) reserved_79_89: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchInfo {
    pub(crate) sale_batch_sending_function: String,
    pub(crate) sale_batch_process_function: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TerminalApInfo {
    pub(crate) phone_number: String,
    pub(crate) ip_octet_1: String,
    pub(crate) ip_octet_2: String,
    pub(crate) ip_octet_3: String,
    pub(crate) ip_octet_4: String,
    pub(crate) port: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaPublicKeyGroup {
    pub(crate) brand_id: String,
    pub(crate) public_keys: Vec<CaPublicKey>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaPublicKey {
    pub(crate) rid: String,
    pub(crate) public_key_index: String,
    pub(crate) public_key_modulus_size: String,
    pub(crate) public_key_modulus: String,
    pub(crate) public_key_exponent_size: String,
    pub(crate) public_key_exponent: String,
    pub(crate) hash_algorithm: String,
    pub(crate) public_key_algorithm_index: String,
    pub(crate) public_key_checksum: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrandInfo {
    pub(crate) brand_id: String,
    pub(crate) aid: String,
    pub(crate) menu_display_name: String,
    pub(crate) validity: String,
    pub(crate) priority: String,
    pub(crate) default_acquirer: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IcCardCompany {
    pub(crate) kid: String,
    pub(crate) fallback: String,
    pub(crate) reserved_5_14: String,
    pub(crate) tables: Vec<IcTable>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "table_id", deny_unknown_fields)]
pub(crate) enum IcTable {
    #[serde(rename = "0")]
    RiskManagement(Box<RiskManagementInfo>),
    #[serde(rename = "9")]
    CommunicationKid(CommunicationKidInfo),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommunicationKidInfo {
    pub(crate) communication_kid: String,
    pub(crate) communication_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RiskManagementInfo {
    pub(crate) brand_id: String,
    pub(crate) acquirer_code: String,
    pub(crate) max_percent: String,
    pub(crate) target_percent: String,
    pub(crate) floor_limit: String,
    pub(crate) threshold: String,
    pub(crate) tac_default: String,
    pub(crate) tac_denial: String,
    pub(crate) tac_online: String,
    pub(crate) force_online: String,
    pub(crate) force_accept: String,
    pub(crate) pin_bypass: String,
    pub(crate) default_ddol: String,
    pub(crate) reserved_155_156: String,
    pub(crate) merchant_type_code: String,
    pub(crate) transaction_type_code: String,
    pub(crate) inquiry: String,
    pub(crate) voice_inquiry: String,
    pub(crate) default_product_code: String,
    pub(crate) brand_individual_info: String,
    pub(crate) reserved_351_372: String,
    pub(crate) apl_info: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnionPayInfo {
    pub(crate) input_selection_info: String,
    pub(crate) pre_authorization_flag: String,
    pub(crate) retail_store_type: String,
    pub(crate) merchant_code: String,
    pub(crate) acquirer_id: String,
    pub(crate) bank_name: String,
    pub(crate) void_pin_flag: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireResponse {
    #[serde(rename = "ErrorCode")]
    pub(crate) error_code: String,
    #[serde(rename = "ResponseID")]
    pub(crate) response_id: String,
    #[serde(rename = "TransactionType")]
    pub(crate) transaction_type: String,
    #[serde(rename = "ResponseTimeStamp")]
    pub(crate) response_timestamp: String,
    #[serde(rename = "DebitInp", skip_serializing_if = "Option::is_none")]
    pub(crate) debit_input: Option<String>,
    #[serde(rename = "TerminalID", skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_id: Option<String>,
    #[serde(rename = "TerminalSeqNo", skip_serializing_if = "Option::is_none")]
    pub(crate) terminal_sequence_number: Option<String>,
    #[serde(rename = "ResponseDateTime", skip_serializing_if = "Option::is_none")]
    pub(crate) response_date_time: Option<String>,
    #[serde(rename = "DLLUpdateAnn", skip_serializing_if = "Option::is_none")]
    pub(crate) dll_update_announcement: Option<String>,
    #[serde(rename = "KCCI_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_card_companies: Option<WireTable>,
    #[serde(rename = "KJSI_01", skip_serializing_if = "Option::is_none")]
    pub(crate) merchant: Option<WireTable>,
    #[serde(rename = "KBAT_01", skip_serializing_if = "Option::is_none")]
    pub(crate) batch: Option<WireTable>,
    #[serde(rename = "KDST_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_terminal_ap: Option<WireTable>,
    #[serde(rename = "KCAK_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_ca_public_keys: Option<WireTable>,
    #[serde(rename = "KBRD_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_brands: Option<WireTable>,
    #[serde(rename = "KICC_01", skip_serializing_if = "Option::is_none")]
    pub(crate) credit_ic_card_companies: Option<WireTable>,
    #[serde(rename = "KGIN_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay: Option<WireTable>,
    #[serde(rename = "GDST_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_terminal_ap: Option<WireTable>,
    #[serde(rename = "GCAK_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_ca_public_keys: Option<WireTable>,
    #[serde(rename = "GBRD_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_brands: Option<WireTable>,
    #[serde(rename = "GICC_01", skip_serializing_if = "Option::is_none")]
    pub(crate) union_pay_ic_card_companies: Option<WireTable>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireTable {
    #[serde(rename = "Length")]
    pub(crate) length: String,
    #[serde(rename = "Data")]
    pub(crate) data: String,
}

impl WireTable {
    pub(crate) fn from_data(data: String) -> Self {
        Self {
            length: data.chars().count().to_string(),
            data,
        }
    }
}
