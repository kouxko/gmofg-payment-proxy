import { Alert, Button, Card, Chip, Spinner, Table } from "@heroui/react";
import type {
  CertificateItemViewModel,
  FieldValidationViewModel,
} from "@/generated/rust-types";

type CertificateValidationSectionProps = {
  localItems: CertificateItemViewModel[];
  isLoading: boolean;
  error?: string;
  validation?: FieldValidationViewModel;
  writePending: boolean;
  validating: boolean;
  onValidate: () => void;
};

export function CertificateValidationSection({
  localItems,
  isLoading,
  error,
  validation,
  writePending,
  validating,
  onValidate,
}: CertificateValidationSectionProps) {
  return (
    <div className="grid grid-cols-[1fr_1.2fr] items-start gap-4 max-[1180px]:grid-cols-1">
      <Card>
        <Card.Header>
          <Card.Title>证书检查结果</Card.Title>
          <Button
            className="ml-auto"
            size="sm"
            variant="outline"
            isDisabled={writePending}
            onPress={onValidate}
          >
            {validating ? "正在检查…" : "重新检查"}
          </Button>
        </Card.Header>
        <Card.Content>
          <CertificateValidationBody
            localItems={localItems}
            isLoading={isLoading}
            error={error}
          />
          {validation && (
            <Alert
              status={validation.valid ? "success" : "danger"}
              className="mt-3"
            >
              {validation.valid
                ? validation.warnings.join("；") || "全部证书检查通过。"
                : Object.values(validation.field_errors).flat().join("；")}
            </Alert>
          )}
        </Card.Content>
      </Card>
      <CertificateTrustCard />
    </div>
  );
}

function CertificateValidationBody({
  localItems,
  isLoading,
  error,
}: Pick<
  CertificateValidationSectionProps,
  "localItems" | "isLoading" | "error"
>) {
  if (isLoading) {
    return (
      <div className="grid min-h-32 place-items-center">
        <Spinner aria-label="正在读取证书检查结果" />
      </div>
    );
  }
  if (error) {
    return (
      <Alert status="danger">
        <Alert.Content>
          <Alert.Title>证书检查结果暂不可用</Alert.Title>
          <Alert.Description>{error}</Alert.Description>
        </Alert.Content>
      </Alert>
    );
  }
  if (localItems.length === 0) {
    return (
      <Alert status="default">
        <Alert.Content>
          <Alert.Title>暂无证书检查结果</Alert.Title>
          <Alert.Description>
            初始化本机证书后，此处显示 Root CA 与服务端证书状态。
          </Alert.Description>
        </Alert.Content>
      </Alert>
    );
  }
  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content aria-label="证书检查结果">
          <Table.Header>
            <Table.Column isRowHeader>检查项</Table.Column>
            <Table.Column>状态</Table.Column>
            <Table.Column>详情</Table.Column>
          </Table.Header>
          <Table.Body>
            {localItems.map((item) => (
              <Table.Row key={item.kind} id={item.kind}>
                <Table.Cell>{item.usage}</Table.Cell>
                <Table.Cell>{item.status_text}</Table.Cell>
                <Table.Cell>{item.subject}</Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table.Content>
      </Table.ScrollContainer>
    </Table>
  );
}

function CertificateTrustCard() {
  const relationships = [
    "客户端 → Proxy（服务端证书）：客户端信任本机导出的公开 Root CA，并校验叶子证书 SAN。",
    "Proxy → 客户端（客户端证书）：仅当入口启用可选或必须客户端认证时，Proxy 才校验客户端证书。",
    "上游服务器 → Proxy（客户端身份）：仅当上游要求 mTLS 时，Proxy 才提交所选 PKCS12 身份。",
    "Proxy → 上游服务器：Proxy 按入口配置的 CA 与主机名策略校验上游服务器。",
  ];
  return (
    <Card>
      <Card.Header>
        <Card.Title>证书信任关系说明</Card.Title>
      </Card.Header>
      <Card.Content className="space-y-3 text-sm">
        {relationships.map((text, index) => (
          <div key={text} className="flex gap-3">
            <Chip size="sm" variant="soft">
              {index + 1}
            </Chip>
            <span>{text}</span>
          </div>
        ))}
      </Card.Content>
    </Card>
  );
}
