// @vitest-environment jsdom

/** Socket Document、协议视图和完整 Hex 分页的行为测试。 */

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { SocketCaptureDocument } from "@/generated/rust-types";
import {
  PaginatedHex,
  ProtocolHexViewer,
  SocketDocumentView,
} from "./socket-byte-viewer";

const documentFixture = {
  schema: {
    id: "all-values",
    version: 1,
    title: "All values",
    fields: [
      { name: "text", type: "string", label: "Text" },
      { name: "amount", type: "int", label: "Amount" },
      { name: "approved", type: "bool", label: "Approved" },
      { name: "binary", type: "blob", label: "Binary" },
      { name: "optional", type: "string", label: "Optional" },
    ],
  },
  values: [
    { type: "string", value: "0200" },
    { type: "int", value: "9223372036854775807" },
    { type: "bool", value: false },
    { type: "blob", value: [0, 65, 255] },
    null,
  ],
} satisfies SocketCaptureDocument;

describe("PaginatedHex", () => {
  it("shows an explicit empty-byte state without inventing a row", () => {
    render(<PaginatedHex bytes={[]} label="Origin Hex" />);

    expect(screen.getByText("空字节流（0 B）")).toBeVisible();
    expect(screen.queryByText(/00000000/)).not.toBeInTheDocument();
  });

  it("formats offsets, hex octets and printable ASCII in 16-byte rows", () => {
    render(<PaginatedHex bytes={[0, 0x41, 0x7e, 0xff]} label="Origin Hex" />);

    expect(screen.getByText(/00000000\s+00 41 7e ff\s+\|\.A~\.\|/)).toBeVisible();
  });

  it("pages through every byte instead of truncating a Blob after 4 KiB", async () => {
    const user = userEvent.setup();
    const bytes = Array.from({ length: 4097 }, (_, index) => index % 256);
    render(<PaginatedHex bytes={bytes} label="Large Blob" />);

    expect(screen.getByText("1 / 2")).toBeVisible();
    expect(screen.queryByText(/00001000/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "下一字节页" }));
    expect(screen.getByText("2 / 2")).toBeVisible();
    expect(screen.getByText(/00001000\s+00/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "上一字节页" }));
    expect(screen.getByText("1 / 2")).toBeVisible();
    expect(screen.getByText(/00000000\s+00 01 02/)).toBeVisible();
  });
});

describe("SocketDocumentView", () => {
  it("renders all four value types and preserves the full i64 string", () => {
    render(<SocketDocumentView document={documentFixture} />);

    expect(screen.getByText("0200")).toBeVisible();
    expect(screen.getByText("9223372036854775807")).toBeVisible();
    expect(screen.getByText("false")).toBeVisible();
    expect(screen.getByRole("region", { name: "Binary Blob 字节" })).toBeVisible();
    expect(screen.getByText("未设置")).toBeVisible();
  });
});

describe("ProtocolHexViewer", () => {
  it("defaults successful custom Display to protocol view while keeping Hex reachable", async () => {
    const user = userEvent.setup();
    render(
      <ProtocolHexViewer
        bytes={[0x30, 0x32]}
        label="Relay Origin"
        display={{ type: "untrusted_html", html: "<p>MTI 0200</p>" }}
      />,
    );

    expect(await screen.findByTitle("Socket 协议安全展示")).toBeVisible();
    const hexTab = screen.getByRole("tab", { name: "Hex" });
    expect(hexTab).toBeEnabled();
    await user.click(hexTab);
    expect(screen.getByRole("region", { name: "Relay Origin Hex" })).toBeVisible();
    expect(screen.queryByTitle("Socket 协议安全展示")).not.toBeInTheDocument();
  });

  it("defaults a Display fallback to Hex even when a decoded Document exists", async () => {
    render(
      <ProtocolHexViewer
        bytes={[0x30, 0x32]}
        label="Relay Written"
        document={documentFixture}
        display={{
          type: "hex_fallback",
          reason: "not_declared",
          diagnostic: null,
        }}
      />,
    );

    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Hex" })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    expect(screen.getByRole("region", { name: "Relay Written Hex" })).toBeVisible();
  });

  it("defaults an undecoded frame to Hex and explains why protocol data is absent", () => {
    render(
      <ProtocolHexViewer
        bytes={[0x30]}
        label="Relay Origin"
        decodeDisabled
      />,
    );

    expect(screen.getByRole("tab", { name: "Hex" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("region", { name: "Relay Origin Hex" })).toBeVisible();
  });

  it("allows a Local Request to prefer its built-in decoded Document", () => {
    render(
      <ProtocolHexViewer
        bytes={[0x30]}
        label="Local Request"
        document={documentFixture}
        preferDocument
      />,
    );

    expect(screen.getByRole("tab", { name: "协议视图" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("9223372036854775807")).toBeVisible();
    expect(screen.getByRole("tab", { name: "Hex" })).toBeEnabled();
  });

  it("defaults an oversized custom Display to Hex without mounting its iframe", () => {
    render(
      <ProtocolHexViewer
        bytes={[0x30]}
        label="Relay Written"
        display={{
          type: "untrusted_html",
          html: `<p>${"x".repeat(128 * 1024)}</p>`,
        }}
      />,
    );

    expect(screen.getByRole("tab", { name: "Hex" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByTitle("Socket 协议安全展示")).toBeNull();
  });

  it("switches tabs with the standard ArrowRight keyboard interaction", async () => {
    const user = userEvent.setup();
    render(
      <ProtocolHexViewer
        bytes={[0x30]}
        label="Keyboard Frame"
        display={{ type: "untrusted_html", html: "<p>safe</p>" }}
      />,
    );

    const protocol = screen.getByRole("tab", { name: "协议视图" });
    protocol.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Hex" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});
