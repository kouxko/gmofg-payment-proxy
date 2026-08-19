import { Button, Modal } from "@heroui/react";
import { Xmark } from "@gravity-ui/icons";
import { FaultPresetsView } from "@/features/faults/faults-view";

interface RuleCreationDialogsProps {
  choiceOpen: boolean;
  faultPresetOpen: boolean;
  onChoiceOpenChange: (open: boolean) => void;
  onFaultPresetOpenChange: (open: boolean) => void;
  onBlankRule: () => void;
  onBodyRule: () => void;
  onFaultPreset: () => void;
  onRuleCreated: (ruleId: string) => void;
}

export function RuleCreationDialogs({
  choiceOpen,
  faultPresetOpen,
  onChoiceOpenChange,
  onFaultPresetOpenChange,
  onBlankRule,
  onBodyRule,
  onFaultPreset,
  onRuleCreated,
}: RuleCreationDialogsProps) {
  return (
    <>
      <Modal isOpen={choiceOpen} onOpenChange={onChoiceOpenChange}>
        <Button className="hidden" aria-hidden="true">
          选择规则创建方式
        </Button>
        <Modal.Backdrop isDismissable>
          <Modal.Container size="lg">
            <Modal.Dialog>
              <Modal.Header className="pr-12">
                <Modal.Heading>新建 HTTP 规则</Modal.Heading>
                <Modal.CloseTrigger aria-label="关闭创建方式选择">
                  <Xmark className="size-4" />
                </Modal.CloseTrigger>
              </Modal.Header>
              <Modal.Body className="grid min-w-0 gap-3 pb-6">
                <Button
                  variant="primary"
                  className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                  onPress={onBlankRule}
                >
                  <span className="min-w-0 flex-1 text-left">
                    <span className="block font-semibold">空白规则</span>
                    <span className="mt-1 block break-words text-sm font-normal opacity-80">
                      自己配置匹配条件和执行动作
                    </span>
                  </span>
                </Button>
                <Button
                  variant="outline"
                  className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                  onPress={onBodyRule}
                >
                  <span className="min-w-0 flex-1 text-left">
                    <span className="block font-semibold">Body 报文规则</span>
                    <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-muted)]">
                      解析 HTTP Body，再按协议字段匹配和改写
                    </span>
                  </span>
                </Button>
                <Button
                  variant="outline"
                  className="h-auto w-full min-w-0 justify-start whitespace-normal px-5 py-4 text-left"
                  onPress={onFaultPreset}
                >
                  <span className="min-w-0 flex-1 text-left">
                    <span className="block font-semibold">从故障预设创建</span>
                    <span className="mt-1 block break-words text-sm font-normal text-[var(--telemetry-muted)]">
                      选择延迟、拒绝、断开、丢弃等模板，再生成普通规则
                    </span>
                  </span>
                </Button>
              </Modal.Body>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal>

      <Modal isOpen={faultPresetOpen} onOpenChange={onFaultPresetOpenChange}>
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
    </>
  );
}
