export type JsonRpcId = string | number;

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: JsonRpcId;
  method: string;
  params: unknown;
}

export interface JsonRpcSuccess {
  jsonrpc: "2.0";
  id: JsonRpcId;
  result: unknown;
}

export interface JsonRpcFailure {
  jsonrpc: "2.0";
  id: JsonRpcId;
  error: { code: number; message: string };
}

export type JsonRpcResponse = JsonRpcSuccess | JsonRpcFailure;

export type DocumentValue =
  | { type: "string"; value: string }
  | { type: "int"; value: string }
  | { type: "bool"; value: boolean }
  | { type: "blob"; value_base64: string };

export type DocumentWire = Record<string, DocumentValue>;

export interface FrameResultNeedMore {
  status: "need_more";
}

export interface FrameResultComplete {
  status: "complete";
  consumed_bytes: number;
}

export type FrameResult = FrameResultNeedMore | FrameResultComplete;
