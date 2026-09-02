/** Converts bytes to canonical, padded RFC 4648 Base64 without third-party code. */
export function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/** Decodes and verifies canonical, padded RFC 4648 Base64. */
export function decodeBase64(value: unknown, field: string): Uint8Array {
  if (typeof value !== "string") throw new Error(`${field} must be a Base64 string`);
  let binary: string;
  try {
    binary = atob(value);
  } catch {
    throw new Error(`${field} must be canonical padded Base64`);
  }
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  if (encodeBase64(bytes) !== value) {
    throw new Error(`${field} must be canonical padded Base64`);
  }
  return bytes;
}
