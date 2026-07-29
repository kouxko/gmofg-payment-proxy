import type { Metadata } from "next";
import { AppProviders } from "@/features/shell/app-providers";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "GMO-FG Payment Proxy",
    template: "%s · GMO-FG Payment Proxy",
  },
  description: "GMO-FG 支付联机测试与协议故障注入工具",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-CN" className="h-full">
      <body className="h-full min-w-0 overflow-hidden antialiased">
        <AppProviders>{children}</AppProviders>
      </body>
    </html>
  );
}
