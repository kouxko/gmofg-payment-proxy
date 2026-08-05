/**
 * 规则编辑器的稳定公开入口。
 *
 * 具体条件、动作和异步草稿管理拆分在同目录模块中；这里保留原导出，避免页面与
 * 测试依赖内部文件结构。所有规则语义仍由 Rust Command 提供，前端只编辑草稿。
 */
export { ActionsEditor } from "./actions-editor";
export { ConditionsEditor } from "./condition-editor";
export {
  actionKind,
  parseRuleByteInput,
  parseRuleHeaderInput,
  requestActionDraft,
  requestConditionDraft,
  requestMatchFieldDraft,
  requestMatchOperatorDraft,
} from "./rule-editor-model";
export type {
  ActionKind,
  ConditionKind,
  RuleDraftChange,
} from "./rule-editor-model";
