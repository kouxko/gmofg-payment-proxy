// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AlertDialog, Button, Drawer, Modal } from "@heroui/react";
import { describe, expect, it } from "vitest";

function OverlayContractHarness() {
  const [alertOpen, setAlertOpen] = useState(false);
  const [alertPending, setAlertPending] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);

  return (
    <>
      <AlertDialog
        isOpen={alertOpen}
        onOpenChange={(open) => {
          if (!open && alertPending) return;
          setAlertOpen(open);
        }}
      >
        <Button>打开确认弹窗</Button>
        <AlertDialog.Backdrop isDismissable={!alertPending}>
          <AlertDialog.Container>
            <AlertDialog.Dialog>
              <AlertDialog.Header>
                <AlertDialog.Heading>确认危险操作</AlertDialog.Heading>
              </AlertDialog.Header>
              <AlertDialog.Body>等待 Rust 返回后才能关闭。</AlertDialog.Body>
              <AlertDialog.Footer>
                <Button
                  slot="close"
                  variant="outline"
                  isDisabled={alertPending}
                >
                  取消确认
                </Button>
                <Button
                  variant="danger"
                  isDisabled={alertPending}
                  onPress={() => setAlertPending(true)}
                >
                  {alertPending ? "正在确认…" : "开始确认"}
                </Button>
              </AlertDialog.Footer>
            </AlertDialog.Dialog>
          </AlertDialog.Container>
        </AlertDialog.Backdrop>
      </AlertDialog>

      <Modal isOpen={modalOpen} onOpenChange={setModalOpen}>
        <Button>打开模态框</Button>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>导入证书</Modal.Heading>
              </Modal.Header>
              <Modal.Body>证书内容由 Rust 读取。</Modal.Body>
              <Modal.Footer>
                <Button slot="close" variant="outline">
                  取消导入
                </Button>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal>

      <Drawer isOpen={drawerOpen} onOpenChange={setDrawerOpen}>
        <Button>打开抽屉</Button>
        <Drawer.Backdrop>
          <Drawer.Content placement="right">
            <Drawer.Dialog>
              <Drawer.Header>
                <Drawer.Heading>处理断点</Drawer.Heading>
              </Drawer.Header>
              <Drawer.Body>断点详情。</Drawer.Body>
              <Drawer.Footer>
                <Button slot="close" variant="outline">
                  取消处理
                </Button>
              </Drawer.Footer>
            </Drawer.Dialog>
          </Drawer.Content>
        </Drawer.Backdrop>
      </Drawer>
    </>
  );
}

describe("HeroUI overlay contracts", () => {
  it("opens and closes Modal and Drawer through direct triggers and Footer close slots", async () => {
    const user = userEvent.setup();
    render(<OverlayContractHarness />);

    await user.click(screen.getByRole("button", { name: "打开模态框" }));
    expect(
      screen.getByRole("dialog", { name: "导入证书" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "取消导入" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "导入证书" }),
      ).not.toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "打开抽屉" }));
    expect(
      screen.getByRole("dialog", { name: "处理断点" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "取消处理" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "处理断点" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("keeps AlertDialog open and disables close actions while Rust is pending", async () => {
    const user = userEvent.setup();
    render(<OverlayContractHarness />);

    await user.click(screen.getByRole("button", { name: "打开确认弹窗" }));
    const dialog = screen.getByRole("alertdialog", {
      name: "确认危险操作",
    });
    expect(dialog).toBeVisible();

    await user.click(screen.getByRole("button", { name: "开始确认" }));
    expect(screen.getByRole("button", { name: "取消确认" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "正在确认…" })).toBeDisabled();

    await user.keyboard("{Escape}");
    expect(
      screen.getByRole("alertdialog", { name: "确认危险操作" }),
    ).toBeVisible();
  });
});
