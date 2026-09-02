import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import type { PageHelpGuide } from "./page-help-content";

type SystemHelpPath = Extract<WorkspacePath, "/certificates" | "/settings">;

export const systemPageHelpGuides: Record<SystemHelpPath, PageHelpGuide> = {
  "/certificates": {
    title: "证书管理",
    summary:
      "只管理客户端 → Proxy 使用的本机 Root CA 与服务端身份；Server CA 和 mTLS 客户端身份按启用固定 Server 的监听配置。",
    recommendedFor:
      "首次部署、局域网 IP/DNS 变化、本机服务端证书到期或客户端不信任 Proxy 时。",
    sections: [
      {
        id: "certificates-first-run",
        title: "首次配置推荐顺序",
        steps: [
          "先到“入口配置”创建需要 TLS 的入口，并确认客户端实际连接的监听 IP 或 DNS。",
          "需要客户端信任 Proxy 时，先确认本机 Root CA 已初始化，再按当前 SAN 签发本机服务端叶子证书。",
          "点击“导出公开 Root CA”取得 PEM .crt，并把它加入需要连接该 Proxy 的受控客户端测试信任库；桌面应用不提供任何私钥导出接口。",
          "执行“重新检查”，确认本机 Root CA、服务端叶子证书和 SAN 有效。",
          "回到“入口配置”；普通上游 TLS 不需要 PKCS12，只有上游明确要求 mTLS 时才在该入口导入客户端身份。",
          "需要自定义上游信任时也在对应入口导入 CA，再点击“测试上游 TLS / mTLS 握手”后启动入口。",
        ],
        notes: [
          "本机 Root CA 仅限受控调试环境，不得用于生产、预生产或真实业务信任体系。",
          "每个安装实例生成独立 Root CA；更换电脑或重置证书后 Root 指纹会改变，客户端必须删除旧 Root 并导入新 Root。",
          "上游 TLS/mTLS 材料与下游 Root CA 相互独立，不能混用。",
        ],
      },
      {
        id: "certificates-file-map",
        title: "证书文件用途与配置位置",
        steps: [
          "Server 客户端身份 PKCS12：仅在目标服务器要求 mTLS 时，到对应监听的固定 Server 配置中导入并自动绑定。",
          "Server CA Bundle：用于验证目标服务器，到对应监听的固定 Server 配置中导入；系统默认信任是否适用也由该监听决定。",
          "本机 Root CA 公共证书：在证书页点击“导出公开 Root CA”取得 PEM .crt，用于让受控客户端信任当前安装实例签发的服务端叶子证书。",
          "Proxy 服务端叶子证书和私钥会根据 SAN 生成并安全保存，不导入客户端应用，也不得导出私钥。",
          "上游客户端 P12、上游 CA、本机 Root CA 和服务端叶子证书用途不同，不能通过改扩展名相互替代。",
        ],
      },
      {
        id: "certificates-app-proxy",
        title: "A.客户端应用App → Proxy",
        steps: [
          "本机 Root CA 用于签发本机服务端叶子证书；需要拦截 TLS 的受控客户端必须信任该公开 CA。",
          "叶子证书 SAN 必须匹配客户端应用实际连接的 Proxy IP/DNS，否则 App → Proxy TLS 会因主机名不匹配失败。",
          "局域网 IP 或 DNS 变化后，先更新对应入口的监听配置，再用新地址重新签发服务端证书并更新入口引用。",
          "仅重新签发叶子证书不会改变本机 Root CA；更换电脑或重置 Root 后，客户端必须删除旧 Root 并导入新 Root。",
          "入口可以选择“不校验客户端证书”“可选”或“必须”；只有后两种模式涉及客户端证书。",
        ],
      },
      {
        id: "certificates-upstream",
        title: "B. Proxy → 上游服务器",
        steps: [
          "只有上游要求 mTLS 时才在目标入口导入对应的客户端身份 PKCS12。",
          "入口弹窗中选择文件并输入密码；空密码就是有效输入，不等同于取消。",
          "在入口中按上游环境选择系统信任或导入官方 CA Bundle；不能用本机下游 Root CA 代替上游 CA。",
          "替换 PKCS12 或上游 CA 后，在同一入口重新执行真实握手测试并重启入口。",
          "不受支持或包含多个私钥身份的 PKCS12 会被拒绝，不会部分写入。",
        ],
      },
      {
        id: "certificates-validate",
        title: "检查证书状态",
        steps: [
          "点击“重新检查”，查看本机 Root CA、服务端叶子 SAN 和到期时间。",
          "逐项读取“状态”和“详情”，不要只看页面顶部总状态。",
          "叶子证书距离到期不足 60 天会显示警告，应在到期前重新签发。",
          "检查主题、用途、SAN、有效期和 SHA-256 指纹，确认导入的是测试环境预期材料。",
          "证书页不会显示密码或私钥；入口导入的 PKCS12 只会被读取、解析并安全保存。",
        ],
      },
      {
        id: "certificates-storage-reset",
        title: "安全存储与重置本机证书",
        steps: [
          "Windows 使用当前登录用户范围 DPAPI，macOS 使用当前登录用户 Keychain 保护私钥、PKCS12 原始字节和密码。",
          "保护或解密失败时 Proxy 会禁止启动，不提供明文回退。",
          "仅在本机证书材料损坏或需要撤销现有 Root 时使用“重置本机证书”。",
          "操作前先停止全部入口；操作会生成新的本机 Root 并按当前 SAN 重新生成服务端叶子证书。",
          "Root 指纹会改变；所有客户端必须删除旧 Root 并导入新导出的公开 Root CA。",
          "升级后必须从客户端和 Windows/macOS 系统信任库中删除旧的 Intercept Proxy TEST ONLY Root CA。",
        ],
      },
      {
        id: "certificates-troubleshooting",
        title: "TLS 失败快速定位",
        steps: [
          "客户端 → Proxy 失败：检查目标 IP/DNS、叶子 SAN、客户端是否信任当前安装实例导出的 Root CA、入口客户端认证模式和电脑防火墙。",
          "Proxy → 上游失败：检查上游 URL 主机名、该入口引用的 P12/上游 CA、系统时间和网络；普通 TLS 不应强制要求客户端 P12。",
          "更换上游材料后结果未变化：在“入口配置”重新测试握手并重启该入口，以创建新的 TLS 上下文。",
          "证书显示有效但仍握手失败：到实时抓包/控制台查看具体 TLS 错误码和发生方向。",
        ],
      },
    ],
  },
  "/settings": {
    title: "系统设置",
    summary:
      "编辑与具体代理入口无关的全局超时、Body、容量、数据和应用策略。监听地址、上游、TLS 与启停均不在此页配置。",
    recommendedFor:
      "调整全局超时和容量、查看 Payload 保存策略，或恢复通用默认设置。",
    sections: [
      {
        id: "settings-boundary",
        title: "系统设置与入口配置的边界",
        steps: [
          "监听地址、监听端口、请求去向、下游 TLS、上游 TLS 和入口启停全部到“入口配置”管理。",
          "一个工作区可建立任意多条入口；不同本地端口或不同上游地址分别建立独立入口。",
          "本页只保存全局超时、Body 上限、HTTP 交换容量、内存容量和 Host 重写策略。",
          "本机 Root CA 与服务端叶子证书在“证书管理”维护；Server CA 与客户端身份在对应监听的固定 Server 配置中维护。",
          "HTTP 故障预设从“拦截规则”的“新建规则”入口选择，最终生成普通规则；Socket 不提供 HTTP 故障预设。",
        ],
      },
      {
        id: "settings-first-run",
        title: "首次设置和证书生成顺序",
        steps: [
          "首次启动先到“工作区”选择工作区，再到“入口配置”新增代理监听；需要固定目标时在同一监听打开“转发到固定 Server”。",
          "需要客户端信任 Proxy 时到证书页初始化并导出 Root CA；Server TLS/mTLS 材料直接在对应监听的固定 Server 配置中导入和测试。",
          "系统设置保持默认即可；只有需要调整全局超时、Body 或容量时才修改本页。",
          "需要修改时直接点击“保存设置”；字段错误会在写入前显示在当前页。",
          "最后回到“入口配置”保存并启动需要的入口；顶部状态栏显示入口总数与活动数，详细过程到“日志”和“抓包”查看。",
        ],
      },
      {
        id: "settings-timeout-capacity",
        title: "超时、Body 与容量",
        steps: [
          "连接、写入和读取超时默认均为 70 秒；根据目标故障场景调整，避免把预期的长延迟误判为网络故障。",
          "单个请求或响应 Body 默认上限为 4 MiB，超过上限会被拒绝并记录 BODY_TOO_LARGE。",
          "最多保留的 HTTP 交换默认 500，逻辑内存上限默认 256 MiB。",
          "提高容量会增加内存使用，长时间压力测试应同时观察控制台运行信息。",
        ],
      },
      {
        id: "settings-save",
        title: "保存和应用",
        steps: [
          "编辑后点击“保存设置”；应用会在写入前完成字段校验，错误会直接显示在当前页。",
          "保存只更新全局设置，不启动、停止或重启任何代理入口。",
          "“放弃更改”恢复到数据库中最新已保存值。",
          "需要改变监听或上游时不要修改本页，直接进入“入口配置”选择对应入口。",
        ],
      },
      {
        id: "settings-defaults-data",
        title: "恢复默认值与数据策略",
        steps: [
          "点击“恢复默认值”并确认后，只会生成默认设置草稿；此时尚未保存或应用。",
          "检查默认超时、Body 和容量，再点击保存。恢复默认值不会删除入口、证书或规则。",
          "点击“清除全部配置与数据”并确认后，会停止入口和设备网络接管，原子删除工作区、入口、规则、设备方案、导入证书与抓包，然后自动重启。",
          "清除操作不可撤销；需要保留的配置应先导出。外观主题属于本机偏好，不随业务配置清除。",
          "Payload 仅保存在内存，HTTP 交换记录随应用重启清空。",
          "规则、配置和证书元数据会持久化；敏感材料由系统当前用户密钥保护。",
          "诊断日志必须脱敏，不应包含 Payload、密码、私钥或 PKCS12 原始内容。",
        ],
      },
      {
        id: "settings-troubleshooting",
        title: "配置保存失败时",
        steps: [
          "超时错误：连接、写入和读取超时必须在允许的正数范围内。",
          "容量错误：HTTP 交换数量、逻辑内存和 Body 上限必须为正数，并避免超过当前测试机可承受范围。",
          "入口端口、上游 URL、SAN 或 TLS 错误不在本页处理，应分别进入“入口配置”或“证书管理”。",
          "保存成功后会刷新当前全局值；入口运行状态不会因为保存系统设置而改变。",
        ],
      },
    ],
  },
};
