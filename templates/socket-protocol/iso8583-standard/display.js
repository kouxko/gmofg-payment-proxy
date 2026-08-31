function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function display({ document }) {
  return `<section class="protocol-document"><h3>ISO 8583:1987 Message</h3><table><tbody><tr><th>MTI</th><td>${escapeHtml(document.message_type ?? "")}</td></tr></tbody></table></section>`;
}

export const upstreamDisplay = display;
export const downstreamDisplay = display;
