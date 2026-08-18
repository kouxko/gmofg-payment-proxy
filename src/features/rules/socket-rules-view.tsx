import { ProtocolRulesView } from "./protocol-rules-view";

export { ProtocolRulesView } from "./protocol-rules-view";

/** Socket 页面仅选择协议类型，共享规则编辑实现保持协议中立。 */
export function SocketRulesView() {
  return <ProtocolRulesView kind="socket" />;
}
