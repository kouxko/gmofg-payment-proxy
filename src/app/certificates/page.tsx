"use client";

import dynamic from "next/dynamic";

const CertificatesView = dynamic(
  () =>
    import("@/features/certificates/certificates-view").then(
      (module) => module.CertificatesView,
    ),
  { ssr: false },
);

export default function CertificatesPage() {
  return <CertificatesView />;
}
