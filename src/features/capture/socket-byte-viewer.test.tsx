// @vitest-environment jsdom

/** Socket 完整 Hex 展示的行为测试。 */

import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SocketByteViewer } from "./socket-byte-viewer";

describe("SocketByteViewer", () => {
  it("shows an explicit empty-byte state without inventing a row", () => {
    render(<SocketByteViewer bytes={[]} label="Origin Hex" />);

    expect(screen.getByText("空字节流（0 B）")).toBeVisible();
    expect(screen.queryByText(/00000000/)).not.toBeInTheDocument();
  });

  it("formats offsets, hex octets and printable ASCII in 16-byte rows", () => {
    render(<SocketByteViewer bytes={[0, 0x41, 0x7e, 0xff]} label="Origin Hex" />);

    expect(screen.getByText(/00000000\s+00 41 7e ff\s+\|\.A~\.\|/)).toBeVisible();
  });

  it("renders every byte without pagination or truncation", () => {
    const bytes = Array.from({ length: 4097 }, (_, index) => index % 256);
    render(<SocketByteViewer bytes={bytes} label="Large Blob" />);

    expect(screen.getByText(/00001000\s+00/)).toBeVisible();
    expect(screen.getByText(/00000000\s+00 01 02/)).toBeVisible();
    expect(screen.queryByRole("button", { name: /字节页/ })).not.toBeInTheDocument();
  });
});
