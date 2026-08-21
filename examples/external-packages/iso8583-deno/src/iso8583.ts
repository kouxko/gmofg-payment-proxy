import { decodeBase64, encodeBase64 } from "./base64.ts";
import type { DocumentValue, DocumentWire, FrameResult } from "./types.ts";

const HEADER_LENGTH = 2;
const MAX_FRAME_LENGTH = 65_535;
const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder("ascii", { fatal: true });

type FieldKind =
  | "fixed_ascii"
  | "fixed_digits"
  | "fixed_amount"
  | "fixed_blob"
  | "ll_ascii"
  | "ll_digits"
  | "lll_ascii"
  | "lll_blob";

export interface FieldSpec {
  readonly number: number;
  readonly name: string;
  readonly label: string;
  readonly kind: FieldKind;
  readonly length: number;
  readonly documentType: "string" | "int" | "blob";
}

/**
 * Deliberately bounded ISO 8583:1987 ASCII financial profile.
 *
 * These are common payment fields, not a claim that every network's DE2-DE128
 * encoding is interchangeable. Extend this table only against an acquirer spec.
 */
export const FIELD_SPECS: readonly FieldSpec[] = [
  spec(2, "primary_account_number", "DE2 Primary Account Number", "ll_digits", 19),
  spec(3, "processing_code", "DE3 Processing Code", "fixed_digits", 6),
  spec(4, "amount", "DE4 Transaction Amount", "fixed_amount", 12, "int"),
  spec(7, "transmission_time", "DE7 Transmission Date And Time", "fixed_digits", 10),
  spec(11, "stan", "DE11 System Trace Audit Number", "fixed_digits", 6),
  spec(12, "local_transaction_time", "DE12 Local Transaction Time", "fixed_digits", 6),
  spec(13, "local_transaction_date", "DE13 Local Transaction Date", "fixed_digits", 4),
  spec(14, "expiration_date", "DE14 Expiration Date", "fixed_digits", 4),
  spec(18, "merchant_type", "DE18 Merchant Type", "fixed_digits", 4),
  spec(22, "pos_entry_mode", "DE22 POS Entry Mode", "fixed_digits", 3),
  spec(23, "card_sequence_number", "DE23 Card Sequence Number", "fixed_digits", 3),
  spec(25, "pos_condition_code", "DE25 POS Condition Code", "fixed_digits", 2),
  spec(32, "acquiring_institution_id", "DE32 Acquiring Institution ID", "ll_digits", 11),
  spec(35, "track_2_data", "DE35 Track 2 Data", "ll_ascii", 37),
  spec(37, "retrieval_reference_number", "DE37 Retrieval Reference Number", "fixed_ascii", 12),
  spec(38, "authorization_id_response", "DE38 Authorization ID Response", "fixed_ascii", 6),
  spec(39, "response_code", "DE39 Response Code", "fixed_ascii", 2),
  spec(41, "terminal_id", "DE41 Terminal ID", "fixed_ascii", 8),
  spec(42, "card_acceptor_id", "DE42 Card Acceptor ID", "fixed_ascii", 15),
  spec(
    43,
    "card_acceptor_name_location",
    "DE43 Card Acceptor Name And Location",
    "fixed_ascii",
    40,
  ),
  spec(49, "currency", "DE49 Transaction Currency", "fixed_digits", 3),
  spec(52, "pin_data", "DE52 PIN Data", "fixed_blob", 8, "blob"),
  spec(53, "security_control_information", "DE53 Security Control Information", "fixed_digits", 16),
  spec(54, "additional_amounts", "DE54 Additional Amounts", "lll_ascii", 120),
  spec(55, "icc_data", "DE55 ICC Data", "lll_blob", 255, "blob"),
  spec(60, "reserved_private_60", "DE60 Reserved Private", "lll_ascii", 999),
  spec(61, "reserved_private_61", "DE61 Reserved Private", "lll_ascii", 999),
  spec(62, "reserved_private_62", "DE62 Reserved Private", "lll_ascii", 999),
  spec(63, "reserved_private_63", "DE63 Reserved Private", "lll_ascii", 999),
  spec(
    64,
    "message_authentication_code",
    "DE64 Message Authentication Code",
    "fixed_blob",
    8,
    "blob",
  ),
  spec(70, "network_management_code", "DE70 Network Management Code", "fixed_digits", 3),
  spec(90, "original_data_elements", "DE90 Original Data Elements", "fixed_digits", 42),
  spec(100, "receiving_institution_id", "DE100 Receiving Institution ID", "ll_digits", 11),
  spec(102, "account_id_1", "DE102 Account ID 1", "ll_ascii", 28),
  spec(103, "account_id_2", "DE103 Account ID 2", "ll_ascii", 28),
  spec(
    128,
    "message_authentication_code_2",
    "DE128 Message Authentication Code",
    "fixed_blob",
    8,
    "blob",
  ),
] as const;

const fieldsByNumber = new Map(FIELD_SPECS.map((field) => [field.number, field]));
const fieldsByName = new Map(FIELD_SPECS.map((field) => [field.name, field]));

function spec(
  number: number,
  name: string,
  label: string,
  kind: FieldKind,
  length: number,
  documentType: FieldSpec["documentType"] = "string",
): FieldSpec {
  return { number, name, label, kind, length, documentType };
}

export function frameBoundary(buffer: Uint8Array): FrameResult {
  if (buffer.length < HEADER_LENGTH) return { status: "need_more" };
  const payloadLength = (requiredByte(buffer, 0) << 8) | requiredByte(buffer, 1);
  const frameLength = HEADER_LENGTH + payloadLength;
  if (frameLength > MAX_FRAME_LENGTH) throw new Error("frame exceeds profile maximum length");
  return buffer.length < frameLength
    ? { status: "need_more" }
    : { status: "complete", consumed_bytes: frameLength };
}

export function decodeFrame(frame: Uint8Array): DocumentWire {
  if (frame.length < HEADER_LENGTH) throw new Error("frame is shorter than its length header");
  const declaredLength = (requiredByte(frame, 0) << 8) | requiredByte(frame, 1);
  if (declaredLength !== frame.length - HEADER_LENGTH) {
    throw new Error("frame length header does not match the received bytes");
  }
  return decodeMessage(frame.subarray(HEADER_LENGTH));
}

export function encodeFrame(document: DocumentWire): Uint8Array {
  validateDocumentKeys(document);
  const message = encodeMessage(document);
  if (message.length + HEADER_LENGTH > MAX_FRAME_LENGTH) {
    throw new Error("encoded ISO 8583 message is too large");
  }
  const frame = new Uint8Array(message.length + HEADER_LENGTH);
  frame[0] = message.length >> 8;
  frame[1] = message.length & 0xff;
  frame.set(message, HEADER_LENGTH);
  return frame;
}

export function renderDocument(document: DocumentWire): string {
  validateDocumentKeys(document);
  const rows: string[] = [];
  const mti = document.message_type;
  if (mti) rows.push(row("MTI", printableValue(mti)));
  for (const field of FIELD_SPECS) {
    const value = document[field.name];
    if (value) rows.push(row(field.label, printableValue(value)));
  }
  const html = `<section class="protocol-document"><h3>ISO 8583:1987 Message</h3><table><tbody>${
    rows.join("")
  }</tbody></table></section>`;
  if (textEncoder.encode(html).length > 128 * 1024) throw new Error("display HTML exceeds 128 KiB");
  return html;
}

function decodeMessage(message: Uint8Array): DocumentWire {
  if (message.length < 12) throw new Error("ISO 8583 message must contain MTI and primary bitmap");
  const document: DocumentWire = {
    message_type: { type: "string", value: readDigits(message, 0, 4, "MTI") },
  };
  let bitmap = message.subarray(4, 12);
  let offset = 12;
  if (bitmapHas(bitmap, 1)) {
    if (message.length < 20) throw new Error("secondary bitmap indicator is set but missing");
    bitmap = message.subarray(4, 20);
    offset = 20;
    if (bitmapHas(bitmap, 65)) throw new Error("tertiary bitmap is outside this profile");
  }
  const lastField = bitmap.length === 16 ? 128 : 64;
  for (let number = 2; number <= lastField; number++) {
    if (!bitmapHas(bitmap, number)) continue;
    const field = fieldsByNumber.get(number);
    if (!field) throw new Error(`DE${number} is not supported by this example profile`);
    const decoded = readField(message, offset, field);
    document[field.name] = decoded.value;
    offset = decoded.nextOffset;
  }
  if (offset !== message.length) {
    throw new Error("message has trailing bytes not described by bitmap");
  }
  return document;
}

function encodeMessage(document: DocumentWire): Uint8Array {
  const mti = requireString(document.message_type, "message_type");
  requireDigits(mti, "message_type");
  if (mti.length !== 4) throw new Error("message_type must contain exactly 4 digits");
  const present = FIELD_SPECS.filter((field) => document[field.name] !== undefined);
  const secondary = present.some((field) => field.number > 64);
  const bitmap = new Uint8Array(secondary ? 16 : 8);
  if (secondary) bitmapSet(bitmap, 1);
  const parts: Uint8Array[] = [ascii(mti, "message_type"), bitmap];
  for (const field of present) {
    bitmapSet(bitmap, field.number);
    parts.push(encodeField(requiredValue(document, field.name), field));
  }
  return concat(parts);
}

function readField(
  message: Uint8Array,
  offset: number,
  field: FieldSpec,
): { value: DocumentValue; nextOffset: number } {
  let length = field.length;
  if (field.kind.startsWith("ll")) {
    const prefixLength = field.kind.startsWith("lll") ? 3 : 2;
    length = Number.parseInt(readDigits(message, offset, prefixLength, `${field.name} length`), 10);
    offset += prefixLength;
    if (length > field.length) throw new Error(`${field.name} length exceeds profile maximum`);
  }
  const bytes = readBytes(message, offset, length, field.name);
  if (field.documentType === "blob") {
    return {
      value: { type: "blob", value_base64: encodeBase64(bytes) },
      nextOffset: offset + length,
    };
  }
  const value = readAscii(bytes, field.name);
  if (field.kind.endsWith("digits") || field.kind === "fixed_amount") {
    requireDigits(value, field.name);
  }
  return {
    value: field.documentType === "int"
      ? { type: "int", value: canonicalInteger(value, field.name) }
      : { type: "string", value },
    nextOffset: offset + length,
  };
}

function encodeField(value: DocumentValue, field: FieldSpec): Uint8Array {
  if (field.documentType === "blob") {
    if (value.type !== "blob") throw new Error(`${field.name} must be a blob`);
    return prefixVariable(decodeBase64(value.value_base64, field.name), field);
  }
  const text = field.documentType === "int"
    ? requireInteger(value, field.name).padStart(field.length, "0")
    : requireString(value, field.name);
  if (field.kind.endsWith("digits") || field.kind === "fixed_amount") {
    requireDigits(text, field.name);
  }
  return prefixVariable(ascii(text, field.name), field);
}

function prefixVariable(payload: Uint8Array, field: FieldSpec): Uint8Array {
  if (payload.length > field.length) {
    throw new Error(`${field.name} length exceeds profile maximum`);
  }
  if (field.kind.startsWith("lll")) {
    return concat([ascii(String(payload.length).padStart(3, "0"), field.name), payload]);
  }
  if (field.kind.startsWith("ll")) {
    return concat([ascii(String(payload.length).padStart(2, "0"), field.name), payload]);
  }
  if (payload.length !== field.length) {
    throw new Error(`${field.name} must contain exactly ${field.length} bytes`);
  }
  return payload;
}

function validateDocumentKeys(document: DocumentWire): void {
  for (const name of Object.keys(document)) {
    if (name !== "message_type" && !fieldsByName.has(name)) {
      throw new Error(`unknown Document field: ${name}`);
    }
  }
}

function requiredValue(document: DocumentWire, name: string): DocumentValue {
  const value = document[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function requireString(value: DocumentValue | undefined, name: string): string {
  if (!value || value.type !== "string") throw new Error(`${name} must be a string`);
  return value.value;
}

function requireInteger(value: DocumentValue, name: string): string {
  if (value.type !== "int" || !/^(0|-?[1-9][0-9]*)$/.test(value.value)) {
    throw new Error(`${name} must be a canonical i64 decimal string`);
  }
  const parsed = BigInt(value.value);
  if (parsed < -(2n ** 63n) || parsed > 2n ** 63n - 1n || parsed < 0n) {
    throw new Error(`${name} must be a non-negative i64 value`);
  }
  return value.value;
}

function canonicalInteger(value: string, name: string): string {
  const normalized = value.replace(/^0+(?=\d)/, "");
  const parsed = BigInt(normalized);
  if (parsed > 2n ** 63n - 1n) throw new Error(`${name} exceeds i64`);
  return parsed.toString();
}

function bitmapHas(bitmap: Uint8Array, field: number): boolean {
  const index = field - 1;
  const byte = bitmap[Math.floor(index / 8)];
  return byte !== undefined && (byte & (1 << (7 - (index % 8)))) !== 0;
}

function bitmapSet(bitmap: Uint8Array, field: number): void {
  const index = field - 1;
  const byteIndex = Math.floor(index / 8);
  const current = bitmap[byteIndex];
  if (current === undefined) throw new Error(`DE${field} requires a secondary bitmap`);
  bitmap[byteIndex] = current | (1 << (7 - (index % 8)));
}

function readDigits(message: Uint8Array, offset: number, length: number, name: string): string {
  const value = readAscii(readBytes(message, offset, length, name), name);
  requireDigits(value, name);
  return value;
}

function requireDigits(value: string, name: string): void {
  if (!/^[0-9]+$/.test(value)) throw new Error(`${name} must contain ASCII digits only`);
}

function readBytes(message: Uint8Array, offset: number, length: number, name: string): Uint8Array {
  if (offset + length > message.length) throw new Error(`${name} exceeds message boundary`);
  return message.subarray(offset, offset + length);
}

function readAscii(bytes: Uint8Array, name: string): string {
  if (bytes.some((byte) => byte > 0x7f)) throw new Error(`${name} must contain ASCII bytes only`);
  return textDecoder.decode(bytes);
}

function ascii(value: string, name: string): Uint8Array {
  if ([...value].some((character) => character.codePointAt(0)! > 0x7f)) {
    throw new Error(`${name} must contain ASCII characters only`);
  }
  return textEncoder.encode(value);
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

function requiredByte(bytes: Uint8Array, index: number): number {
  const value = bytes[index];
  if (value === undefined) throw new Error(`missing byte ${index}`);
  return value;
}

function printableValue(value: DocumentValue): string {
  return value.type === "blob"
    ? `[binary ${decodeBase64(value.value_base64, "display").length} bytes]`
    : String(value.value);
}

function row(label: string, value: string): string {
  return `<tr><th>${escapeHtml(label)}</th><td>${escapeHtml(value)}</td></tr>`;
}

function escapeHtml(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
