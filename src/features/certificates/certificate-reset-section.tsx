import { Alert, AlertDialog, Button } from "@heroui/react";
import { Shield, TrashBin } from "@gravity-ui/icons";

type CertificateResetSectionProps = {
  isOpen: boolean;
  resetPending: boolean;
  canReset: boolean;
  writePending: boolean;
  onOpenChange: (open: boolean) => void;
  onReset: () => void;
};

export function CertificateResetSection({
  isOpen,
  resetPending,
  canReset,
  writePending,
  onOpenChange,
  onReset,
}: CertificateResetSectionProps) {
  return (
    <Alert status="danger">
      <Alert.Indicator>
        <Shield className="size-5" />
      </Alert.Indicator>
      <Alert.Content>
        <Alert.Title>恢复固定测试证书并重签叶子证书</Alert.Title>
        <Alert.Description>
          将恢复内置的固定测试 Root CA，并按当前 SAN 重新生成本机叶子证书。
          已信任该固定 Root CA 的客户端无需重新导入；仅所有代理入口均已停止时可执行。
        </Alert.Description>
      </Alert.Content>
      <AlertDialog isOpen={isOpen} onOpenChange={onOpenChange}>
        <Button variant="danger" isDisabled={!canReset || writePending}>
          <TrashBin className="size-4" />
          恢复固定测试证书
        </Button>
        <AlertDialog.Backdrop>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header>
                <AlertDialog.Heading>确认恢复固定测试证书？</AlertDialog.Heading>
              </AlertDialog.Header>
              <AlertDialog.Body>
                将恢复应用内置的固定测试 Root CA，并重新生成本机服务端叶子证书。
                Root CA 指纹保持不变，但当前连接会被停止，因此仅可在所有代理入口停止后执行。
              </AlertDialog.Body>
              <AlertDialog.Footer>
                <Button
                  slot="close"
                  variant="outline"
                  isDisabled={resetPending}
                >
                  取消
                </Button>
                <Button
                  variant="danger"
                  isDisabled={resetPending}
                  onPress={onReset}
                >
                  {resetPending ? "正在重置…" : "确认重置"}
                </Button>
              </AlertDialog.Footer>
            </AlertDialog.Dialog>
          </AlertDialog.Container>
        </AlertDialog.Backdrop>
      </AlertDialog>
    </Alert>
  );
}
