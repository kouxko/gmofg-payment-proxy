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
        <Alert.Title>重置本机 Root CA 并重签叶子证书</Alert.Title>
        <Alert.Description>
          将生成新的本机 Root CA，并按当前 SAN 重新生成叶子证书。
          客户端必须删除旧 Root 并导入新 Root；仅所有代理入口均已停止时可执行。
        </Alert.Description>
      </Alert.Content>
      <AlertDialog isOpen={isOpen} onOpenChange={onOpenChange}>
        <Button variant="danger" isDisabled={!canReset || writePending}>
          <TrashBin className="size-4" />
          重置本机证书
        </Button>
        <AlertDialog.Backdrop>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header>
                <AlertDialog.Heading>确认重置本机证书？</AlertDialog.Heading>
              </AlertDialog.Header>
              <AlertDialog.Body>
                将生成新的本机 Root CA，并重新生成服务端叶子证书。Root CA
                指纹会改变，客户端必须删除旧 Root 并导入新 Root；当前连接也会被停止。
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
