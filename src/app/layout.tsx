import type { Metadata } from "next";
import { AppProviders } from "@/features/shell/app-providers";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "网络代理工具",
    template: "%s · 网络代理工具",
  },
  description: "双向 mTLS 联机测试与协议故障注入工具",
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
