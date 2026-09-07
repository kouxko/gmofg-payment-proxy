"use client";

/**
 * 当前页面的内置中文使用说明 Drawer。
 *
 * 帮助内容是静态教学文本，不参与任何业务判断。打开和关闭 Drawer 不导航、不
 * 重载 WebView，也不会改变当前页面的选中项、表单草稿或 Rust 订阅。
 */

import { Accordion, Alert, Button, Drawer } from "@heroui/react";
import { BookOpen } from "@gravity-ui/icons";
import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import { pageHelpGuides } from "./page-help-content";

export function PageHelp({ pathname }: { pathname: WorkspacePath }) {
  const guide = pageHelpGuides[pathname];

  return (
    <Drawer>
      <Button
        isIconOnly
        size="sm"
        variant="ghost"
        aria-label={`打开${guide.title}使用说明`}
      >
        <BookOpen className="size-4" />
      </Button>
      <Drawer.Backdrop>
        <Drawer.Content placement="right">
          <Drawer.Dialog>
            <Drawer.Header>
              <Drawer.Heading>{guide.title}使用说明</Drawer.Heading>
            </Drawer.Header>
            <Drawer.Body className="space-y-4">
              <Alert status="accent">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Title>本页用途</Alert.Title>
                  <Alert.Description>{guide.summary}</Alert.Description>
                </Alert.Content>
              </Alert>
              <div className="rounded-xl bg-[var(--telemetry-soft)] p-4 text-sm">
                <div className="font-semibold">适合在什么情况下使用</div>
                <p className="mt-1 text-[var(--telemetry-muted)]">
                  {guide.recommendedFor}
                </p>
              </div>
              <Accordion
                defaultExpandedKeys={[guide.sections[0].id]}
                aria-label={`${guide.title}详细操作说明`}
              >
                {guide.sections.map((section) => (
                  <Accordion.Item key={section.id} id={section.id}>
                    <Accordion.Heading>
                      <Accordion.Trigger>
                        {section.title}
                        <Accordion.Indicator />
                      </Accordion.Trigger>
                    </Accordion.Heading>
                    <Accordion.Panel>
                      <Accordion.Body className="space-y-3 pb-4">
                        {section.description && (
                          <p className="text-sm text-[var(--telemetry-muted)]">
                            {section.description}
                          </p>
                        )}
                        <ol className="list-decimal space-y-2 pl-5 text-sm">
                          {section.steps.map((step) => (
                            <li key={step} className="pl-1 leading-6">
                              {step}
                            </li>
                          ))}
                        </ol>
                        {section.notes && section.notes.length > 0 && (
                          <div className="rounded-xl border border-[var(--telemetry-line)] p-3">
                            <div className="text-sm font-semibold">注意</div>
                            <ul className="mt-2 list-disc space-y-1 pl-5 text-sm text-[var(--telemetry-muted)]">
                              {section.notes.map((note) => (
                                <li key={note} className="leading-6">
                                  {note}
                                </li>
                              ))}
                            </ul>
                          </div>
                        )}
                      </Accordion.Body>
                    </Accordion.Panel>
                  </Accordion.Item>
                ))}
              </Accordion>
            </Drawer.Body>
            <Drawer.Footer>
              <Button slot="close" variant="primary">
                关闭使用说明
              </Button>
            </Drawer.Footer>
          </Drawer.Dialog>
        </Drawer.Content>
      </Drawer.Backdrop>
    </Drawer>
  );
}
