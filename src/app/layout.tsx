import type { Metadata } from "next";
import { AppProviders } from "@/features/shell/app-providers";
import { ThemeProvider } from "@/features/theme/theme-provider";
import "./globals.css";

/**
 * Next.js 静态导出的根布局。
 *
 * Tauri 最终只加载这份静态页面。RootLayout 只设置语言、元数据和全屏容器；
 * AppProviders 再把它接到持久工作区。不要在此读取文件、网络或业务配置。
 */

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
        <ThemeProvider>
          <AppProviders>{children}</AppProviders>
        </ThemeProvider>
      </body>
    </html>
  );
}
