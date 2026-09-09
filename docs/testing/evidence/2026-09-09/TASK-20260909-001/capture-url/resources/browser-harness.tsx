"use client";
import { useState } from "react";
import { Button } from "@heroui/react";
import { ExchangeObservationList } from "@/features/capture/exchange-observation-list";
import { exchangePage, httpExchangeRecord } from "@/features/capture/exchange-observation-test-fixture";
export default function CaptureUrlSmoke() {
  const [latest, setLatest] = useState(false);
  const [selected, setSelected] = useState("");
  const record = httpExchangeRecord();
  if (latest) record.events.push({ event: "received", observed_at: "2026-09-09T01:00:00Z", direction: "upstream", context: { protocol: "http", header: `POST /api/pay?token=a%2Fb+c&empty=&long=${"x".repeat(2048)} HTTP/1.1`, body: "", body_is_utf8: true }, document: null, display: null });
  return <main><Button onPress={() => setLatest(true)}>追加 App 原始长请求</Button><output aria-label="选中连接">{selected}</output><ExchangeObservationList page={{ ...exchangePage(), rows: [record, exchangePage().rows[1]] }} loading={false} selectedId={selected} onSelect={(row) => setSelected(row.exchange_id)} onPage={() => {}} onRetry={() => {}} onClear={() => {}} /></main>;
}
