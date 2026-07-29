import { Suspense } from "react";
import { RulesView } from "@/features/rules/rules-view";

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
