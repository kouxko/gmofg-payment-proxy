# Design

## Source of truth

- Status: Draft
- Last refreshed: 2026-08-13
- Primary product surfaces: Tauri desktop application, persistent Next.js workspace shell, Rust-owned proxy configuration and runtime state.
- Evidence reviewed: `docs/requirements.md`, `docs/user-operation-guide.md`, `design-qa.md`, `docs/assets/ui/*.png`, `src/app/globals.css`, `src/features/shell/app-shell.tsx`, `src/features/shell/workspace-navigation.tsx`, `src/features/listeners/listeners-view.tsx`, `src/features/listeners/socket-listener-settings.tsx`.
- Feature contract: `.omx/plans/socket-protocol-scripting.md`, `templates/socket-protocol/API.md`, `templates/socket-protocol/AUTHORING.md`.

## Brand

- Personality: technical, calm, precise and operational; closer to a network diagnostic console than a consumer application.
- Trust signals: Rust-returned validation state, explicit data direction, exact package/schema versions, visible fallbacks, stable status colors and non-ambiguous destructive actions.
- Avoid: decorative dashboards, hidden automation, protocol auto-detection claims, unexplained abbreviations and HTTP-specific controls on Socket screens.

## Product goals

- Goals: let test engineers import versioned Socket protocol packages, inspect their declared capabilities and bind one exact package version to a Scripted Socket Listener.
- Goals: make Direct byte forwarding and Scripted parsing visibly different modes without weakening the existing Direct path.
- Goals: expose Document fields as rule variables and render protocol-defined Document HTML with a reliable Hex fallback.
- Non-goals: editing Rhai source inside the application, automatically guessing a package from traffic, downloading packages from a marketplace, or letting scripts access files/network/processes.
- Success signals: users can identify the active package/version and fallback behavior before starting a Listener; invalid packages cannot become selectable; Socket rules never show HTTP-only fields.

## Personas and jobs

- Primary personas: payment/network integration engineers, QA engineers and protocol developers working with private TCP systems.
- User jobs: install a package ZIP, understand whether it compiled, inspect its Document fields, enable/disable it, bind it to a Listener, independently control Decode/Encode in each direction and diagnose a frame/decode/encode failure.
- Key contexts of use: desktop test environments on Windows/macOS, often while comparing captured bytes, rules and live connection status.

## Information architecture

- Primary navigation: retain the persistent left navigation and add `协议包` immediately after `入口配置`.
- Core route: proposed `/protocol-packages`, application-scoped rather than Workspace-scoped because multiple Workspaces may reference the same immutable `package_id + version`.
- Core screens: protocol package list with version-detail Dialog, ZIP import validation Dialog, Socket Listener processing card, Socket Document/Hex inspection, Schema-driven rule field selection.
- Content hierarchy: identity/status first, then installed versions, capabilities, Schema, validation diagnostics and usage references. Source content is not an application surface.

### Package ownership and portability

- Installed protocol packages are application-scoped and immutable by `package_id + version`.
- Protocol packages have no content digest or digital-signature contract. An installed `package_id + version` is reused and cannot be overwritten; changed content requires a new version.
- `.intercept-workspace` embeds every exact package version referenced by that Workspace's Listeners.
- `.intercept-config` embeds all installed packages and their application-level enabled/disabled state.
- Workspace import installs missing embedded packages globally but leaves newly installed scripts disabled until explicit review and enablement.
- Workspace configuration stores only package ID/version references; package enablement is not a Workspace-owned field.
- Target machines revalidate and recompile packages. Compiled ASTs, caches, installation paths and runtime connections are never portable artifacts.

### Protocol package page

The page lists packages by immutable package ID. Selecting a row opens a version-detail Dialog:

```text
┌ 协议包 ───────────────────────────────────────── [导入 ZIP] ┐
│ 搜索 / 状态筛选                                             │
├─────────────────────────────────────────────────────────────┤
│ ISO 8583 ASCII Standard   iso8583-ascii   2 个版本   已启用 │
│ Custom TLV                custom-tlv      1 个版本   已停用 │
└─────────────────────────────────────────────────────────────┘

点击 ISO 8583：
┌ 版本与详情 Dialog ──────────────────────────────────────────┐
│ 1.1.0  1.0.0  │ 身份 | 能力 | Schema | 校验 | 使用者       │
│                │ 当前精确版本详情                           │
└─────────────────────────────────────────────────────────────┘
```

- Package row: package name, ID, enabled/disabled state, installed version count and usage count.
- Dialog version list: every installed version for that package ID, sorted by SemVer.
- Detail header: name, exact ID/version, API version, Schema ID/version, status and primary actions.
- Overview: identity, installed time, declared capabilities and fixed fallback behavior.
- Document fields: table of name, label and type; field name uses monospace. Protocol-specific presence requirements stay in scripts.
- Entry points: four Hook rows plus Display, showing function, script, required/optional, declared and compile state.
- Validation: only successfully validated packages can be installed. Import errors stay in the import Dialog and never create a package row.
- Users: Workspace, Listener name and runtime status; each row navigates to the owning Listener.
- The app does not request, return, display or edit Rhai source. Version details contain declarations and bounded diagnostics only.
- Disable action is blocked when any referencing Listener is running; the Users tab becomes the actionable blocker list.
- Delete action is blocked while any saved Listener references the version, including stopped Listeners.
- Stopped references remain intact when a package is disabled and navigate back to this page when startup is blocked.

### ZIP import flow

```text
选择 ZIP -> Rust 解包和校验 -> 导入预览 -> 安装
```

- File selection uses the native dialog through Rust.
- Rust validates ZIP paths/limits, strict Manifest/Schema, every Rhai module, syntax, parameters and return types before returning a preview.
- Preview shows package identity, Schema field count, declared capabilities, validation result and conflicts, but never source.
- Same `id + version` cannot overwrite an installed package; a new version installs alongside it.
- An existing exact `id + version` is reused without comparing or overwriting its installed content.
- Failed validation leaves no partially installed package, file or compiled cache. A syntax error cannot be installed.
- Enabling is explicit after a successful install; importing does not silently rebind Listeners.

### Socket Listener processing card

Place a `Socket 数据处理` card after `Socket 上游目标` and before direction-specific TLS cards:

```text
Socket 数据处理
  处理方式       Direct（原始字节） | Scripted（协议包）

  协议包         ISO 8583 ASCII Standard · 1.0.0
                 iso8583-ascii-standard · Schema v1 · 可用

  Upstream       [开] Decode   App -> Server
                 [开] Encode   App -> Server（同时启用 Display）

  Downstream     [开] Decode   Server -> App
                 [关] Encode   Server -> App（关闭时 Display 关闭）

  回退说明       Encode 关闭 -> origin 原字节 + Hex
                 Display 未声明或失败 -> Hex

  [查看协议包详情]
```

- Direct mode hides package selection and capability switches and states that no Frame/Document/rules are produced.
- Scripted mode requires one enabled, valid, API-compatible exact package version.
- Each direction exposes independent Decode and Encode switches. Display has no independent switch and follows that direction's Encode switch.
- Decode off/Encode on is valid: Encode receives origin and an empty Schema-bound Document, so a script may ignore Document and transform raw bytes.
- A missing Encode entry is shown as unsupported and cannot be enabled; Rust rejects forged configurations.
- Package ID/version and Schema version remain visible below the human name so similarly named packages cannot be confused.
- A running Listener cannot change processing mode, package version or capability switches; stop it first.
- Transparent Socket TLS can expose encrypted opaque bytes. The card must explain that scripts only parse bytes visible after the configured TLS boundary.

### Capture/session and rules

- Socket message detail keeps Hex always reachable. Display is a non-blocking observation after Encode determines output bytes. When Display succeeds, provide an in-content `协议视图 / Hex` switch rather than adding HTTP Header/Body tabs.
- Display missing or failed selects Hex and shows a compact diagnostic without affecting forwarding.
- Socket rule fields come from the selected package Document Schema. Field picker shows label, variable name and type.
- Socket rules do not render HTTP Method, URL/Path, Query, Header, Cookie, Status Code, JSONPath or HTTP Body controls, including disabled placeholders.
- A rule that modifies a Document field is unavailable unless the relevant output direction has Encode enabled.

## Design principles

- Direction before capability: always show Upstream as App -> Server and Downstream as Server -> App near directional switches.
- Declare fallback at the control: users should know `Encode off -> origin + Hex` and `Display unavailable/failed -> Hex` without opening documentation.
- Immutable identity: name is descriptive; package ID/version and Schema version are the operational identity.
- Validation is Rust evidence: the frontend renders package and Listener ViewModels and does not infer compilation or compatibility.
- Preserve raw evidence: protocol HTML supplements Hex and never removes access to original Frame bytes.
- Tradeoff: the version-detail Dialog favors diagnostic density over a simplified consumer-style layout because its users are protocol authors and operators.

## Visual language

- Color: reuse `--telemetry-*` tokens; accent teal for selection/primary action, green for valid/enabled, warning for fallback/degraded and danger for invalid/destructive.
- Typography: existing Segoe UI/Microsoft YaHei stack; monospace only for IDs, versions, function names, field names and byte values.
- Spacing/layout rhythm: existing `p-5`, `gap-4/5`, bordered Cards and Dialog layouts; avoid isolated oversized cards and unnecessary blank areas.
- Shape/radius/elevation: HeroUI defaults plus existing rounded cards, `telemetry-line` borders and restrained `shadow-sm`.
- Motion: HeroUI defaults only; respect the existing reduced-motion global rule.
- Imagery/iconography: Gravity UI icons already used by the shell; add one archive/code-oriented icon for the protocol package navigation item.

## Components

- Existing components to reuse: HeroUI Button, Card, Chip, Table, Tabs, Select, Switch, Alert, Modal, AlertDialog, Spinner, Tooltip and native-dialog command patterns.
- New/changed components: `ProtocolPackagesView`, `ProtocolPackageDialog`, `ProtocolPackageVersionList`, `DocumentSchemaTable`, `ProtocolPackageImportDialog`, `SocketProcessingCard`, `SocketDocumentView` and Schema-aware Socket rule fields.
- Variants and states: enabled, disabled, incompatible API after a host upgrade, referenced, validating/importing, empty and command failure. Invalid imports are Dialog errors, not installed-package states.
- Token/component ownership: visual tokens remain in `src/app/globals.css`; business status and validation text come from Rust ViewModels.

## Accessibility

- Target standard: keyboard-operable desktop UI with semantic HeroUI controls and readable status text independent of color.
- Keyboard/focus behavior: package rows, version list and Tabs must be keyboard selectable; detail/import/enable/delete dialogs restore focus to their trigger.
- Contrast/readability: reuse current light/dark tokens; IDs and diagnostics must wrap or scroll without clipping.
- Screen-reader semantics: status Chips require adjacent textual meaning; direction switches include full accessible labels such as `启用 Upstream Encode，App 到 Server`.
- Reduced motion and sensory considerations: preserve the existing `prefers-reduced-motion` behavior; no animated traffic visualization is required.

## Responsive behavior

- Supported breakpoints/devices: desktop-first Tauri window; existing shell hides side navigation at 1025px.
- Layout adaptations: the package list stays single-column; the version-detail Dialog becomes a scrollable stacked version/detail layout below 900px.
- Touch/hover differences: do not hide required actions behind hover; table/list row selection remains available by click, keyboard and touch.

## Interaction states

- Loading: show a Spinner with stable page header; detail loading stays inside the open Dialog.
- Empty: explain that protocol packages are only needed for Scripted Socket processing and provide `导入 ZIP`; do not imply Direct mode is incomplete.
- Error: import errors remain in the import Dialog with Rust error code and affected file/function; invalid content is not installed.
- Success: show install/enable/disable confirmation and refresh usage/status from Rust.
- Disabled: disabled packages remain inspectable; stopped Listener references remain saved, but new selection and Listener startup are blocked with an enable-package action.
- Referenced: running references block disable; all saved references block delete. Show Workspace/Listener links and runtime status beside the blocked action.
- Offline/slow network: not applicable to local package import; long validation remains cancellable only if the Rust command supports cancellation.

## Content voice

- Tone: direct Chinese operational language with English protocol identifiers where they are the actual API term.
- Terminology: UI uses `代理入口`, `协议包`, `Upstream（App -> Server）`, `Downstream（Server -> App）`, `Document`, `Frame`, `Hex` and `原始字节` consistently.
- Microcopy rules: never say a package was auto-detected; distinguish `已安装`, `已启用`, `校验通过`, `正在使用` and `Listener 运行中`.

## Implementation constraints

- Framework/styling system: Next.js 16 static export, React 19, HeroUI 3, Tailwind CSS 4 and Gravity UI icons.
- Architecture: Rust owns ZIP I/O, validation, package persistence, Rhai compilation, capability state and usage lookup; frontend only displays generated ViewModels and submits commands.
- Navigation: add the route to the in-memory `WorkspaceNavigation`/`WorkspaceContent` path union; do not force a WebView reload.
- Design-token constraints: reuse `telemetry-*`; do not create a protocol-package-only color system or custom replacements for HeroUI controls.
- Performance constraints: package list uses summaries; version detail/Schema/usage load on selection. Never send source, the ZIP or compiled AST to the frontend.
- Compatibility constraints: Direct Socket relay remains the default and must not depend on Rhai availability.
- Portability constraints: Workspace export embeds referenced packages; whole-app export embeds all packages and enabled state. All import validation and conflict resolution remains atomic in Rust.
- Test/screenshot expectations: navigation, empty/loading/error, import validation, version coexistence, usage blocking, Listener capability gating and Document/Hex fallback require UI contract tests; final layout requires a fresh packaged-app screenshot at supported desktop dimensions.

## Closed decisions

- Package details do not include a source viewer. Protocol source stays in the author-owned ZIP and is never part of the frontend ViewModel.
- Display has no Listener switch of its own; it follows Encode independently for Upstream and Downstream.
