use crate::model::{
    BatchInfo, BrandInfo, CaPublicKey, CaPublicKeyGroup, CardCompany, CardRange,
    CommunicationKidInfo, DllDocument, IcCardCompany, IcTable, MerchantInfo, RiskManagementInfo,
    TerminalApInfo, UnionPayInfo, WireResponse, WireTable,
};

const CREDIT: &str = "0001";
const UNION_PAY: &str = "0002";
const CONNECTION_TEST: &str = "0000";

pub(crate) fn decode(input: &str) -> Result<DllDocument, String> {
    let wire: WireResponse = serde_json::from_str(input)
        .map_err(|error| format!("GMO-FG Payment DLL response is not valid JSON: {error}"))?;
    wire_to_document(wire)
}

pub(crate) fn encode(document: &DllDocument) -> Result<String, String> {
    let wire = document_to_wire(document)?;
    serde_json::to_string(&wire)
        .map_err(|error| format!("cannot serialize GMO-FG Payment DLL response: {error}"))
}

fn wire_to_document(wire: WireResponse) -> Result<DllDocument, String> {
    let WireResponse {
        error_code,
        response_id,
        transaction_type,
        response_timestamp,
        debit_input,
        terminal_id,
        terminal_sequence_number,
        response_date_time,
        dll_update_announcement,
        credit_card_companies,
        merchant,
        batch,
        credit_terminal_ap,
        credit_ca_public_keys,
        credit_brands,
        credit_ic_card_companies,
        union_pay,
        union_pay_terminal_ap,
        union_pay_ca_public_keys,
        union_pay_brands,
        union_pay_ic_card_companies,
    } = wire;

    let document = DllDocument {
        error_code,
        response_id,
        transaction_type,
        response_timestamp,
        debit_input,
        terminal_id,
        terminal_sequence_number,
        response_date_time,
        dll_update_announcement,
        credit_card_companies: credit_card_companies
            .map(|table| parse_table(&table, "KCCI_01", parse_card_companies))
            .transpose()?,
        merchant: merchant
            .map(|table| parse_table(&table, "KJSI_01", parse_merchant))
            .transpose()?,
        batch: batch
            .map(|table| parse_table(&table, "KBAT_01", parse_batch))
            .transpose()?,
        credit_terminal_ap: credit_terminal_ap
            .map(|table| parse_table(&table, "KDST_01", parse_terminal_ap))
            .transpose()?,
        credit_ca_public_keys: credit_ca_public_keys
            .map(|table| parse_table(&table, "KCAK_01", parse_ca_public_key_groups))
            .transpose()?,
        credit_brands: credit_brands
            .map(|table| parse_table(&table, "KBRD_01", parse_brands))
            .transpose()?,
        credit_ic_card_companies: credit_ic_card_companies
            .map(|table| parse_table(&table, "KICC_01", parse_ic_card_companies))
            .transpose()?,
        union_pay: union_pay
            .map(|table| parse_table(&table, "KGIN_01", parse_union_pay))
            .transpose()?,
        union_pay_terminal_ap: union_pay_terminal_ap
            .map(|table| parse_table(&table, "GDST_01", parse_terminal_ap))
            .transpose()?,
        union_pay_ca_public_keys: union_pay_ca_public_keys
            .map(|table| parse_table(&table, "GCAK_01", parse_ca_public_key_groups))
            .transpose()?,
        union_pay_brands: union_pay_brands
            .map(|table| parse_table(&table, "GBRD_01", parse_brands))
            .transpose()?,
        union_pay_ic_card_companies: union_pay_ic_card_companies
            .map(|table| parse_table(&table, "GICC_01", parse_ic_card_companies))
            .transpose()?,
    };
    validate_transaction_shape(&document)?;
    Ok(document)
}

fn document_to_wire(document: &DllDocument) -> Result<WireResponse, String> {
    validate_transaction_shape(document)?;
    Ok(WireResponse {
        error_code: document.error_code.clone(),
        response_id: document.response_id.clone(),
        transaction_type: document.transaction_type.clone(),
        response_timestamp: document.response_timestamp.clone(),
        debit_input: document.debit_input.clone(),
        terminal_id: document.terminal_id.clone(),
        terminal_sequence_number: document.terminal_sequence_number.clone(),
        response_date_time: document.response_date_time.clone(),
        dll_update_announcement: document.dll_update_announcement.clone(),
        credit_card_companies: document
            .credit_card_companies
            .as_deref()
            .map(encode_card_companies)
            .transpose()?
            .map(WireTable::from_data),
        merchant: document
            .merchant
            .as_ref()
            .map(encode_merchant)
            .transpose()?
            .map(WireTable::from_data),
        batch: document
            .batch
            .as_ref()
            .map(encode_batch)
            .transpose()?
            .map(WireTable::from_data),
        credit_terminal_ap: document
            .credit_terminal_ap
            .as_ref()
            .map(encode_terminal_ap)
            .transpose()?
            .map(WireTable::from_data),
        credit_ca_public_keys: document
            .credit_ca_public_keys
            .as_deref()
            .map(encode_ca_public_key_groups)
            .transpose()?
            .map(WireTable::from_data),
        credit_brands: document
            .credit_brands
            .as_deref()
            .map(encode_brands)
            .transpose()?
            .map(WireTable::from_data),
        credit_ic_card_companies: document
            .credit_ic_card_companies
            .as_deref()
            .map(encode_ic_card_companies)
            .transpose()?
            .map(WireTable::from_data),
        union_pay: document
            .union_pay
            .as_ref()
            .map(encode_union_pay)
            .transpose()?
            .map(WireTable::from_data),
        union_pay_terminal_ap: document
            .union_pay_terminal_ap
            .as_ref()
            .map(encode_terminal_ap)
            .transpose()?
            .map(WireTable::from_data),
        union_pay_ca_public_keys: document
            .union_pay_ca_public_keys
            .as_deref()
            .map(encode_ca_public_key_groups)
            .transpose()?
            .map(WireTable::from_data),
        union_pay_brands: document
            .union_pay_brands
            .as_deref()
            .map(encode_brands)
            .transpose()?
            .map(WireTable::from_data),
        union_pay_ic_card_companies: document
            .union_pay_ic_card_companies
            .as_deref()
            .map(encode_ic_card_companies)
            .transpose()?
            .map(WireTable::from_data),
    })
}

fn validate_transaction_shape(document: &DllDocument) -> Result<(), String> {
    let has_connection = document.terminal_id.is_some()
        || document.terminal_sequence_number.is_some()
        || document.response_date_time.is_some()
        || document.dll_update_announcement.is_some();
    let has_credit_only = document.credit_card_companies.is_some()
        || document.batch.is_some()
        || document.credit_terminal_ap.is_some()
        || document.credit_ca_public_keys.is_some()
        || document.credit_brands.is_some()
        || document.credit_ic_card_companies.is_some();
    let has_union_only = document.union_pay.is_some()
        || document.union_pay_terminal_ap.is_some()
        || document.union_pay_ca_public_keys.is_some()
        || document.union_pay_brands.is_some()
        || document.union_pay_ic_card_companies.is_some();

    match document.transaction_type.as_str() {
        CONNECTION_TEST => {
            if document.debit_input.is_some()
                || document.merchant.is_some()
                || has_credit_only
                || has_union_only
            {
                return Err("TransactionType 0000 contains DLL parameter tables".to_owned());
            }
        }
        CREDIT => {
            if has_connection || has_union_only {
                return Err(
                    "TransactionType 0001 contains connection-test or UnionPay fields".to_owned(),
                );
            }
            if document.debit_input.is_none() {
                return Err("TransactionType 0001 is missing DebitInp".to_owned());
            }
        }
        UNION_PAY => {
            if has_connection || has_credit_only {
                return Err(
                    "TransactionType 0002 contains connection-test or Credit fields".to_owned(),
                );
            }
            if document.debit_input.is_none() {
                return Err("TransactionType 0002 is missing DebitInp".to_owned());
            }
        }
        other => {
            return Err(format!(
                "unsupported GMO-FG Payment DLL TransactionType: {other}"
            ));
        }
    }
    Ok(())
}

fn parse_table<T>(
    table: &WireTable,
    name: &str,
    parser: fn(&str, &str) -> Result<T, String>,
) -> Result<T, String> {
    let declared = table
        .length
        .parse::<usize>()
        .map_err(|_| format!("{name}.Length is not an unsigned decimal integer"))?;
    let actual = table.data.chars().count();
    if declared != actual {
        return Err(format!(
            "{name}.Length declares {declared} characters but Data contains {actual}"
        ));
    }
    parser(&table.data, name)
}

fn parse_card_companies(data: &str, name: &str) -> Result<Vec<CardCompany>, String> {
    split_terminated(data, '@', name)?
        .into_iter()
        .enumerate()
        .map(|(index, record)| {
            let label = format!("{name} record {index}");
            let chars = at_least(record, 37, &label)?;
            expect_id(&chars, '1', &label)?;
            if (chars.len() - 37) % 32 != 0 {
                return Err(format!(
                    "{label} has {} trailing characters; card ranges require 32 each",
                    chars.len() - 37
                ));
            }
            let mut card_ranges = Vec::new();
            for start in (37..chars.len()).step_by(32) {
                card_ranges.push(CardRange {
                    from: slice(&chars, start, start + 16),
                    to: slice(&chars, start + 16, start + 32),
                });
            }
            Ok(CardCompany {
                kid: slice(&chars, 1, 4),
                acquirer_code: slice(&chars, 4, 11),
                card_issuer_name: slice(&chars, 11, 21),
                manual_input: slice(&chars, 21, 25),
                input_selection_info: slice(&chars, 25, 28),
                password_input: slice(&chars, 28, 32),
                payment_type_selection: slice(&chars, 32, 37),
                card_ranges,
            })
        })
        .collect()
}

fn encode_card_companies(records: &[CardCompany]) -> Result<String, String> {
    let mut data = String::new();
    for (index, record) in records.iter().enumerate() {
        let label = format!("KCCI_01 record {index}");
        data.push('1');
        push_width(&mut data, &record.kid, 3, &format!("{label}.kid"))?;
        push_width(
            &mut data,
            &record.acquirer_code,
            7,
            &format!("{label}.acquirer_code"),
        )?;
        push_width(
            &mut data,
            &record.card_issuer_name,
            10,
            &format!("{label}.card_issuer_name"),
        )?;
        push_width(
            &mut data,
            &record.manual_input,
            4,
            &format!("{label}.manual_input"),
        )?;
        push_width(
            &mut data,
            &record.input_selection_info,
            3,
            &format!("{label}.input_selection_info"),
        )?;
        push_width(
            &mut data,
            &record.password_input,
            4,
            &format!("{label}.password_input"),
        )?;
        push_width(
            &mut data,
            &record.payment_type_selection,
            5,
            &format!("{label}.payment_type_selection"),
        )?;
        for (range_index, range) in record.card_ranges.iter().enumerate() {
            let range_label = format!("{label}.card_ranges[{range_index}]");
            push_width(&mut data, &range.from, 16, &format!("{range_label}.from"))?;
            push_width(&mut data, &range.to, 16, &format!("{range_label}.to"))?;
        }
        data.push('@');
    }
    Ok(data)
}

fn parse_merchant(data: &str, name: &str) -> Result<MerchantInfo, String> {
    let chars = exact(data, 90, name)?;
    expect_char(&chars, 0, '2', name)?;
    expect_char(&chars, 41, '@', name)?;
    expect_char(&chars, 42, '3', name)?;
    expect_char(&chars, 89, '@', name)?;
    Ok(MerchantInfo {
        phone_number_1: slice(&chars, 1, 21),
        phone_number_2: slice(&chars, 21, 41),
        info_1: slice(&chars, 43, 56),
        info_2: slice(&chars, 56, 79),
        reserved_79_89: slice(&chars, 79, 89),
    })
}

fn encode_merchant(value: &MerchantInfo) -> Result<String, String> {
    let mut data = String::from("2");
    push_width(
        &mut data,
        &value.phone_number_1,
        20,
        "KJSI_01.phone_number_1",
    )?;
    push_width(
        &mut data,
        &value.phone_number_2,
        20,
        "KJSI_01.phone_number_2",
    )?;
    data.push_str("@3");
    push_width(&mut data, &value.info_1, 13, "KJSI_01.merchant_info_1")?;
    push_width(&mut data, &value.info_2, 23, "KJSI_01.merchant_info_2")?;
    push_width(
        &mut data,
        &value.reserved_79_89,
        10,
        "KJSI_01.reserved_79_89",
    )?;
    data.push('@');
    Ok(data)
}

fn parse_batch(data: &str, name: &str) -> Result<BatchInfo, String> {
    let chars = exact(data, 9, name)?;
    expect_char(&chars, 0, '4', name)?;
    expect_char(&chars, 8, '@', name)?;
    Ok(BatchInfo {
        sale_batch_sending_function: slice(&chars, 1, 7),
        sale_batch_process_function: slice(&chars, 7, 8),
    })
}

fn encode_batch(value: &BatchInfo) -> Result<String, String> {
    let mut data = String::from("4");
    push_width(
        &mut data,
        &value.sale_batch_sending_function,
        6,
        "KBAT_01.sale_batch_sending_function",
    )?;
    push_width(
        &mut data,
        &value.sale_batch_process_function,
        1,
        "KBAT_01.sale_batch_process_function",
    )?;
    data.push('@');
    Ok(data)
}

fn parse_terminal_ap(data: &str, name: &str) -> Result<TerminalApInfo, String> {
    let chars = exact(data, 39, name)?;
    expect_char(&chars, 0, '5', name)?;
    expect_char(&chars, 38, '@', name)?;
    Ok(TerminalApInfo {
        phone_number: slice(&chars, 1, 21),
        ip_octet_1: slice(&chars, 21, 24),
        ip_octet_2: slice(&chars, 24, 27),
        ip_octet_3: slice(&chars, 27, 30),
        ip_octet_4: slice(&chars, 30, 33),
        port: slice(&chars, 33, 38),
    })
}

fn encode_terminal_ap(value: &TerminalApInfo) -> Result<String, String> {
    let mut data = String::from("5");
    push_width(
        &mut data,
        &value.phone_number,
        20,
        "terminal_ap.phone_number",
    )?;
    push_width(&mut data, &value.ip_octet_1, 3, "terminal_ap.ip_octet_1")?;
    push_width(&mut data, &value.ip_octet_2, 3, "terminal_ap.ip_octet_2")?;
    push_width(&mut data, &value.ip_octet_3, 3, "terminal_ap.ip_octet_3")?;
    push_width(&mut data, &value.ip_octet_4, 3, "terminal_ap.ip_octet_4")?;
    push_width(&mut data, &value.port, 5, "terminal_ap.port")?;
    data.push('@');
    Ok(data)
}

fn parse_ca_public_key_groups(data: &str, name: &str) -> Result<Vec<CaPublicKeyGroup>, String> {
    split_terminated(data, '@', name)?
        .into_iter()
        .enumerate()
        .map(|(index, record)| parse_ca_public_key_group(record, &format!("{name} group {index}")))
        .collect()
}

fn parse_ca_public_key_group(record: &str, label: &str) -> Result<CaPublicKeyGroup, String> {
    let chars = at_least(record, 4, label)?;
    expect_id(&chars, '6', label)?;
    let declared = chars[3]
        .to_digit(10)
        .ok_or_else(|| format!("{label} public-key count is not one decimal digit"))?
        as usize;
    if (chars.len() - 4) % 602 != 0 {
        return Err(format!(
            "{label} has {} public-key characters; keys require 602 each",
            chars.len() - 4
        ));
    }
    let actual = (chars.len() - 4) / 602;
    if actual != declared {
        return Err(format!(
            "{label} declares {declared} CA public keys but contains {actual}"
        ));
    }
    let mut public_keys = Vec::with_capacity(actual);
    for start in (4..chars.len()).step_by(602) {
        public_keys.push(parse_ca_public_key(&chars[start..start + 602]));
    }
    Ok(CaPublicKeyGroup {
        brand_id: slice(&chars, 1, 3),
        public_keys,
    })
}

fn parse_ca_public_key(chars: &[char]) -> CaPublicKey {
    CaPublicKey {
        rid: slice(chars, 0, 10),
        public_key_index: slice(chars, 10, 12),
        public_key_modulus_size: slice(chars, 12, 16),
        public_key_modulus: slice(chars, 16, 512),
        public_key_exponent_size: slice(chars, 512, 514),
        public_key_exponent: slice(chars, 514, 520),
        hash_algorithm: slice(chars, 520, 560),
        public_key_algorithm_index: slice(chars, 560, 562),
        public_key_checksum: slice(chars, 562, 602),
    }
}

fn encode_ca_public_key_groups(groups: &[CaPublicKeyGroup]) -> Result<String, String> {
    let mut data = String::new();
    for (index, group) in groups.iter().enumerate() {
        let label = format!("CA public-key group {index}");
        if group.public_keys.len() > 9 {
            return Err(format!("{label} contains more than 9 public keys"));
        }
        data.push('6');
        push_width(&mut data, &group.brand_id, 2, &format!("{label}.brand_id"))?;
        let count = u32::try_from(group.public_keys.len())
            .map_err(|_| format!("{label} public-key count does not fit one digit"))?;
        data.push(char::from_digit(count, 10).expect("count <= 9"));
        for (key_index, key) in group.public_keys.iter().enumerate() {
            encode_ca_public_key(&mut data, key, &format!("{label}.public_keys[{key_index}]"))?;
        }
        data.push('@');
    }
    Ok(data)
}

fn encode_ca_public_key(data: &mut String, key: &CaPublicKey, label: &str) -> Result<(), String> {
    push_width(data, &key.rid, 10, &format!("{label}.rid"))?;
    push_width(
        data,
        &key.public_key_index,
        2,
        &format!("{label}.public_key_index"),
    )?;
    push_width(
        data,
        &key.public_key_modulus_size,
        4,
        &format!("{label}.public_key_modulus_size"),
    )?;
    push_width(
        data,
        &key.public_key_modulus,
        496,
        &format!("{label}.public_key_modulus"),
    )?;
    push_width(
        data,
        &key.public_key_exponent_size,
        2,
        &format!("{label}.public_key_exponent_size"),
    )?;
    push_width(
        data,
        &key.public_key_exponent,
        6,
        &format!("{label}.public_key_exponent"),
    )?;
    push_width(
        data,
        &key.hash_algorithm,
        40,
        &format!("{label}.hash_algorithm"),
    )?;
    push_width(
        data,
        &key.public_key_algorithm_index,
        2,
        &format!("{label}.public_key_algorithm_index"),
    )?;
    push_width(
        data,
        &key.public_key_checksum,
        40,
        &format!("{label}.public_key_checksum"),
    )
}

fn parse_brands(data: &str, name: &str) -> Result<Vec<BrandInfo>, String> {
    split_terminated(data, '@', name)?
        .into_iter()
        .enumerate()
        .map(|(index, record)| {
            let label = format!("{name} record {index}");
            let chars = exact(record, 66, &label)?;
            expect_id(&chars, '7', &label)?;
            Ok(BrandInfo {
                brand_id: slice(&chars, 1, 3),
                aid: slice(&chars, 3, 35),
                menu_display_name: slice(&chars, 35, 55),
                validity: slice(&chars, 55, 56),
                priority: slice(&chars, 56, 59),
                default_acquirer: slice(&chars, 59, 66),
            })
        })
        .collect()
}

fn encode_brands(records: &[BrandInfo]) -> Result<String, String> {
    let mut data = String::new();
    for (index, record) in records.iter().enumerate() {
        let label = format!("brand record {index}");
        data.push('7');
        push_width(&mut data, &record.brand_id, 2, &format!("{label}.brand_id"))?;
        push_width(&mut data, &record.aid, 32, &format!("{label}.aid"))?;
        push_width(
            &mut data,
            &record.menu_display_name,
            20,
            &format!("{label}.menu_display_name"),
        )?;
        push_width(&mut data, &record.validity, 1, &format!("{label}.validity"))?;
        push_width(&mut data, &record.priority, 3, &format!("{label}.priority"))?;
        push_width(
            &mut data,
            &record.default_acquirer,
            7,
            &format!("{label}.default_acquirer"),
        )?;
        data.push('@');
    }
    Ok(data)
}

fn parse_ic_card_companies(data: &str, name: &str) -> Result<Vec<IcCardCompany>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let body = data
        .strip_suffix("@@")
        .ok_or_else(|| format!("{name} must end with @@"))?;
    if body.is_empty() {
        return Ok(Vec::new());
    }
    body.split("@@")
        .enumerate()
        .map(|(index, group)| parse_ic_card_company(group, &format!("{name} group {index}")))
        .collect()
}

fn parse_ic_card_company(group: &str, label: &str) -> Result<IcCardCompany, String> {
    let chars = at_least(group, 14, label)?;
    expect_id(&chars, '8', label)?;
    let mut tables = Vec::new();
    let mut position = 14;
    while position < chars.len() {
        let table_id = chars[position];
        let width = match table_id {
            '0' => 412,
            '9' => 10,
            other => {
                return Err(format!(
                    "{label} contains unknown nested table ID {other} at character {position}"
                ));
            }
        };
        if position + width > chars.len() {
            return Err(format!(
                "{label} nested table {table_id} at character {position} is truncated"
            ));
        }
        let table_chars = &chars[position..position + width];
        tables.push(match table_id {
            '0' => IcTable::RiskManagement(Box::new(parse_risk(table_chars, label)?)),
            '9' => IcTable::CommunicationKid(parse_communication_kid(table_chars)),
            _ => unreachable!(),
        });
        position += width;
        if position < chars.len() {
            expect_char(&chars, position, '@', label)?;
            position += 1;
        }
    }
    Ok(IcCardCompany {
        kid: slice(&chars, 1, 4),
        fallback: slice(&chars, 4, 5),
        reserved_5_14: slice(&chars, 5, 14),
        tables,
    })
}

fn parse_communication_kid(chars: &[char]) -> CommunicationKidInfo {
    CommunicationKidInfo {
        communication_kid: slice(chars, 1, 4),
        communication_id: slice(chars, 4, 10),
    }
}

fn parse_risk(chars: &[char], label: &str) -> Result<RiskManagementInfo, String> {
    expect_id(chars, '0', label)?;
    Ok(RiskManagementInfo {
        brand_id: slice(chars, 1, 3),
        acquirer_code: slice(chars, 3, 10),
        max_percent: slice(chars, 10, 12),
        target_percent: slice(chars, 12, 14),
        floor_limit: slice(chars, 14, 20),
        threshold: slice(chars, 20, 26),
        tac_default: slice(chars, 26, 66),
        tac_denial: slice(chars, 66, 106),
        tac_online: slice(chars, 106, 146),
        force_online: slice(chars, 146, 147),
        force_accept: slice(chars, 147, 148),
        pin_bypass: slice(chars, 148, 149),
        default_ddol: slice(chars, 149, 155),
        reserved_155_156: slice(chars, 155, 156),
        merchant_type_code: slice(chars, 156, 160),
        transaction_type_code: slice(chars, 160, 161),
        inquiry: slice(chars, 161, 162),
        voice_inquiry: slice(chars, 162, 163),
        default_product_code: slice(chars, 163, 170),
        brand_individual_info: slice(chars, 170, 351),
        reserved_351_372: slice(chars, 351, 372),
        apl_info: slice(chars, 372, 412),
    })
}

fn encode_ic_card_companies(companies: &[IcCardCompany]) -> Result<String, String> {
    let mut data = String::new();
    for (index, company) in companies.iter().enumerate() {
        let label = format!("IC card company {index}");
        data.push('8');
        push_width(&mut data, &company.kid, 3, &format!("{label}.kid"))?;
        push_width(
            &mut data,
            &company.fallback,
            1,
            &format!("{label}.fallback"),
        )?;
        push_width(
            &mut data,
            &company.reserved_5_14,
            9,
            &format!("{label}.reserved_5_14"),
        )?;
        for (table_index, table) in company.tables.iter().enumerate() {
            if table_index > 0 {
                data.push('@');
            }
            match table {
                IcTable::RiskManagement(value) => {
                    encode_risk(&mut data, value, &format!("{label}.tables[{table_index}]"))?;
                }
                IcTable::CommunicationKid(value) => {
                    encode_communication_kid(
                        &mut data,
                        value,
                        &format!("{label}.tables[{table_index}]"),
                    )?;
                }
            }
        }
        data.push_str("@@");
    }
    Ok(data)
}

fn encode_communication_kid(
    data: &mut String,
    value: &CommunicationKidInfo,
    label: &str,
) -> Result<(), String> {
    data.push('9');
    push_width(
        data,
        &value.communication_kid,
        3,
        &format!("{label}.communication_kid"),
    )?;
    push_width(
        data,
        &value.communication_id,
        6,
        &format!("{label}.communication_id"),
    )
}

fn encode_risk(data: &mut String, value: &RiskManagementInfo, label: &str) -> Result<(), String> {
    data.push('0');
    let fields: [(&str, usize, &str); 21] = [
        (&value.brand_id, 2, "brand_id"),
        (&value.acquirer_code, 7, "acquirer_code"),
        (&value.max_percent, 2, "max_percent"),
        (&value.target_percent, 2, "target_percent"),
        (&value.floor_limit, 6, "floor_limit"),
        (&value.threshold, 6, "threshold"),
        (&value.tac_default, 40, "tac_default"),
        (&value.tac_denial, 40, "tac_denial"),
        (&value.tac_online, 40, "tac_online"),
        (&value.force_online, 1, "force_online"),
        (&value.force_accept, 1, "force_accept"),
        (&value.pin_bypass, 1, "pin_bypass"),
        (&value.default_ddol, 6, "default_ddol"),
        (&value.reserved_155_156, 1, "reserved_155_156"),
        (&value.merchant_type_code, 4, "merchant_type_code"),
        (&value.transaction_type_code, 1, "transaction_type_code"),
        (&value.inquiry, 1, "inquiry"),
        (&value.voice_inquiry, 1, "voice_inquiry"),
        (&value.default_product_code, 7, "default_product_code"),
        (&value.brand_individual_info, 181, "brand_individual_info"),
        (&value.reserved_351_372, 21, "reserved_351_372"),
    ];
    for (field, width, name) in fields {
        push_width(data, field, width, &format!("{label}.{name}"))?;
    }
    push_width(data, &value.apl_info, 40, &format!("{label}.apl_info"))
}

fn parse_union_pay(data: &str, name: &str) -> Result<UnionPayInfo, String> {
    let chars = exact(data, 57, name)?;
    expect_char(&chars, 0, 'A', name)?;
    expect_char(&chars, 56, '@', name)?;
    Ok(UnionPayInfo {
        input_selection_info: slice(&chars, 1, 4),
        pre_authorization_flag: slice(&chars, 4, 5),
        retail_store_type: slice(&chars, 5, 9),
        merchant_code: slice(&chars, 9, 24),
        acquirer_id: slice(&chars, 24, 35),
        bank_name: slice(&chars, 35, 55),
        void_pin_flag: slice(&chars, 55, 56),
    })
}

fn encode_union_pay(value: &UnionPayInfo) -> Result<String, String> {
    let mut data = String::from("A");
    let fields: [(&str, usize, &str); 7] = [
        (&value.input_selection_info, 3, "input_selection_info"),
        (&value.pre_authorization_flag, 1, "pre_authorization_flag"),
        (&value.retail_store_type, 4, "retail_store_type"),
        (&value.merchant_code, 15, "merchant_code"),
        (&value.acquirer_id, 11, "acquirer_id"),
        (&value.bank_name, 20, "bank_name"),
        (&value.void_pin_flag, 1, "void_pin_flag"),
    ];
    for (field, width, name) in fields {
        push_width(&mut data, field, width, &format!("KGIN_01.{name}"))?;
    }
    data.push('@');
    Ok(data)
}

fn split_terminated<'a>(
    data: &'a str,
    terminator: char,
    label: &str,
) -> Result<Vec<&'a str>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let body = data
        .strip_suffix(terminator)
        .ok_or_else(|| format!("{label} must end with {terminator}"))?;
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let records: Vec<_> = body.split(terminator).collect();
    if records.iter().any(|record| record.is_empty()) {
        return Err(format!("{label} contains an empty record"));
    }
    Ok(records)
}

fn exact(input: &str, expected: usize, label: &str) -> Result<Vec<char>, String> {
    let chars: Vec<_> = input.chars().collect();
    if chars.len() != expected {
        return Err(format!(
            "{label} requires {expected} characters but contains {}",
            chars.len()
        ));
    }
    Ok(chars)
}

fn at_least(input: &str, minimum: usize, label: &str) -> Result<Vec<char>, String> {
    let chars: Vec<_> = input.chars().collect();
    if chars.len() < minimum {
        return Err(format!(
            "{label} requires at least {minimum} characters but contains {}",
            chars.len()
        ));
    }
    Ok(chars)
}

fn expect_id(chars: &[char], expected: char, label: &str) -> Result<(), String> {
    expect_char(chars, 0, expected, label)
}

fn expect_char(chars: &[char], index: usize, expected: char, label: &str) -> Result<(), String> {
    match chars.get(index) {
        Some(actual) if *actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "{label} character {index} must be {expected} but is {actual}"
        )),
        None => Err(format!("{label} is missing character {index}")),
    }
}

fn slice(chars: &[char], start: usize, end: usize) -> String {
    chars[start..end].iter().collect()
}

fn push_width(data: &mut String, field: &str, width: usize, label: &str) -> Result<(), String> {
    let actual = field.chars().count();
    if actual != width {
        return Err(format!(
            "{label} requires {width} characters but contains {actual}"
        ));
    }
    data.push_str(field);
    Ok(())
}
