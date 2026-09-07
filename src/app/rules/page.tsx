import { Suspense } from "react";
import { RulesView } from "@/features/rules/rules-view";

/**
 * 规则页的 Next.js 静态入口。
 * RulesView 会读取工作区 searchParams 预填“从会话创建的规则”，因此用 Suspense
 * 提供最小加载占位；实际规则数据和校验仍来自 Rust。
 */

export default function RulesPage() {
  return (
    <Suspense
      fallback={
        <div className="grid h-full place-items-center text-sm">
          正在读取规则草稿…
        </div>
      }
    >
      <RulesView />
    </Suspense>
  );
}
