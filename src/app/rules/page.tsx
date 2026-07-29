"use client";

import dynamic from "next/dynamic";
import { Suspense } from "react";

const RulesView = dynamic(
  () =>
    import("@/features/rules/rules-view").then(
      (module) => module.RulesView,
    ),
  { ssr: false },
);

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
