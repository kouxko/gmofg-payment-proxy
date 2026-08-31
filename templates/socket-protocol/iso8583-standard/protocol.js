const HEADER_BYTES = 2;

function frame({ buffer }) {
  if (buffer.length < HEADER_BYTES) {
    return { status: "need_more", requiredBytes: HEADER_BYTES };
  }
  const payloadBytes = (buffer[0] << 8) | buffer[1];
  const consumedBytes = HEADER_BYTES + payloadBytes;
  if (buffer.length < consumedBytes) {
    return { status: "need_more", requiredBytes: consumedBytes };
  }
  return { status: "complete", consumedBytes };
}

function decode({ input }) {
  if (input.length < HEADER_BYTES + 4) {
    throw new Error("ISO 8583 frame is shorter than its MTI");
  }
  const declared = (input[0] << 8) | input[1];
  if (declared !== input.length - HEADER_BYTES) {
    throw new Error("ISO 8583 frame length header does not match the input");
  }
  return {
    message_type: String.fromCharCode(input[2], input[3], input[4], input[5]),
  };
}

function encode({ originalInput, document }) {
  const messageType = document.message_type;
  if (typeof messageType !== "string" || messageType.length !== 4) {
    throw new Error("ISO 8583 message_type must contain exactly four characters");
  }
  const output = new Uint8Array(originalInput);
  for (let index = 0; index < 4; index += 1) {
    const code = messageType.charCodeAt(index);
    if (code > 0x7f) throw new Error("ISO 8583 message_type must be ASCII");
    output[HEADER_BYTES + index] = code;
  }
  return output;
}

export const upstreamFrame = frame;
export const downstreamFrame = frame;
export const upstreamDecode = decode;
export const downstreamDecode = decode;
export const upstreamEncode = encode;
export const downstreamEncode = encode;
