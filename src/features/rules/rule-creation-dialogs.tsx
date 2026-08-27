import { Button, Modal } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import { FaultPresetsView } from "@/features/faults/faults-view";
import type { RuleCreationOption } from "./rule-creation-capability";

interface RuleCreationDialogsProps {
  choiceOpen: boolean;
  faultPresetOpen: boolean;
  onChoiceOpenChange: (open: boolean) => void;
  onFaultPresetOpenChange: (open: boolean) => void;
  http: RuleCreationOption;
  body: RuleCreationOption;
  socket: RuleCreationOption;
  onHttpRule: () => void;
  onBodyRule: () => void;
  onSocketRule: () => void;
  onFaultPreset: () => void;
  onRuleCreated: (ruleId: string) => void;
}

export function RuleCreationDialogs({
  choiceOpen,
  faultPresetOpen,
  onChoiceOpenChange,
  onFaultPresetOpenChange,
  http,
  body,
  socket,
  onHttpRule,
  onBodyRule,
  onSocketRule,
  onFaultPreset,
  onRuleCreated,
}: RuleCreationDialogsProps) {
  return (
    <>
      {choiceOpen && (
        <Modal
          isOpen={choiceOpen}
          onOpenChange={(open) => {
            // 打开动作由列表顶部按钮负责；忽略 DialogTrigger 在选项点击后产生的
            // 回开事件，避免业务路由已经切换但旧弹窗再次覆盖编辑器。
            if (!open) onChoiceOpenChange(false);
          }}
        >
          <Button className="hidden" aria-hidden="true">
            选择规则创建方式
          </Button>
          <Modal.Backdrop isDismissable>
            <Modal.Container size="lg">
              <Modal.Dialog>
                <Modal.Header className="pr-12">
                  <Modal.Heading>新建规则</Modal.Heading>
                  <Modal.CloseTrigger aria-label="关闭创建方式选择">
                    <Xmark className="size-4" />
                  </Modal.CloseTrigger>
                </Modal.Header>
                <Modal.Body className="grid min-w-0 gap-3 pb-6">
                  <Button
                    slot="close"
                    variant="primary"
                    className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                    isDisabled={http.disabled}
                    onPress={onHttpRule}
                  >
                    <span className="min-w-0 flex-1 text-left">
                      <span className="block font-semibold">HTTP 规则</span>
                      <span className="mt-1 block break-words text-sm font-normal opacity-80">
                        绑定 HTTP 入口，再配置请求、响应或 TLS 匹配与动作
                      </span>
                      {http.reason && <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-danger)]">{http.reason}</span>}
                    </span>
                  </Button>
                  <Button
                    slot="close"
                    variant="outline"
                    isDisabled={body.disabled}
                    className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                    onPress={onBodyRule}
                  >
                    <span className="min-w-0 flex-1 text-left">
                      <span className="block font-semibold">Body 报文规则</span>
                      <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-muted)]">
                        解析 HTTP Body，再按协议字段匹配和改写
                      </span>
                      {body.reason && <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-danger)]">{body.reason}</span>}
                    </span>
                  </Button>
                  <Button
                    slot="close"
                    variant="outline"
                    isDisabled={socket.disabled}
                    className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                    onPress={onSocketRule}
                  >
                    <span className="min-w-0 flex-1 text-left">
                      <span className="block font-semibold">Socket 报文规则</span>
                      <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-muted)]">
                        解析 Socket 报文，再按协议字段匹配和改写
                      </span>
                      {socket.reason && <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-danger)]">{socket.reason}</span>}
                    </span>
                  </Button>
                  <Button
                    slot="close"
                    variant="outline"
                    isDisabled={http.disabled}
                    className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                    onPress={onFaultPreset}
                  >
                    <span className="min-w-0 flex-1 text-left">
                      <span className="block font-semibold">从故障预设创建</span>
                      <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-muted)]">
                        选择延迟、拒绝、断开、丢弃等模板，再生成普通规则
                      </span>
                      {http.reason && <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-danger)]">{http.reason}</span>}
                    </span>
                  </Button>
                </Modal.Body>
              </Modal.Dialog>
            </Modal.Container>
          </Modal.Backdrop>
        </Modal>
      )}

      {faultPresetOpen && (
        <Modal
          isOpen={faultPresetOpen}
          onOpenChange={(open) => {
            if (!open) onFaultPresetOpenChange(false);
          }}
        >
          <Button className="hidden" aria-hidden="true">
            打开故障预设
          </Button>
          <Modal.Backdrop isDismissable>
            <Modal.Container size="cover" scroll="inside">
              <Modal.Dialog>
                <Modal.Header className="pr-12">
                  <Modal.Heading>从故障预设创建规则</Modal.Heading>
                  <Modal.CloseTrigger aria-label="关闭故障预设">
                    <Xmark className="size-4" />
                  </Modal.CloseTrigger>
                </Modal.Header>
                <Modal.Body className="min-h-0 p-0">
                  <FaultPresetsView onRuleCreated={onRuleCreated} />
                </Modal.Body>
              </Modal.Dialog>
            </Modal.Container>
          </Modal.Backdrop>
        </Modal>
      )}
    </>
  );
}
