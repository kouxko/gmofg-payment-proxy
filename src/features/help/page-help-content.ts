import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import { systemPageHelpGuides } from "./system-page-help-guides";
import { diagnosticPageHelpGuides } from "./diagnostic-page-help-guide";
import { generalPageHelpGuides } from "./general-page-help-guides";
import { protocolPackageHelpGuide } from "./protocol-package-help-guide";
import type { PageHelpGuide } from "./page-help-types";

export type { PageHelpGuide, PageHelpSection } from "./page-help-types";

/**
 * 各业务页面的内置操作手册数据。
 *
 * 这里只保存面向用户的静态说明，不能复制 Rust 的状态机或校验逻辑。文字中的
 * 默认值、动作语义和验收边界应与 docs/requirements.md 保持同步。
 */

export const pageHelpGuides: Record<WorkspacePath, PageHelpGuide> = {
  "/workspaces": generalPageHelpGuides["/workspaces"],
  "/listeners": generalPageHelpGuides["/listeners"],
  ...protocolPackageHelpGuide,
  "/android-network": generalPageHelpGuides["/android-network"],
  ...diagnosticPageHelpGuides,
  "/capture": {
    title: "实时抓包",
    summary:
      "在同一张运行记录表中查看经过代理的 HTTP 与 Socket Exchange，适合在操作客户端应用的同时观察完整连接和规则结果。",
    recommendedFor:
      "定位某个 App 连接使用了哪种协议、在哪个读写阶段失败，以及 App、Proxy 与 Server 之间实际收到和发送的内容。",
    sections: [
      {
        id: "capture-start",
        title: "开始抓包与读取列表",
        steps: [
          "先在“入口配置”启动目标入口，再进入本页；页面会持续接收 HTTP 与 Socket 的 Exchange 事件。",
          "页面只有一张运行记录表；“协议”列明确标识 HTTP 或 SOCKET，不需要切换页签或查看两个区域。",
          "每行对应一个 App connection，并显示建立时间、对端、收到/发送/失败计数、最终结果和 Exchange ID。",
          "使用“上一页/下一页”查看当前工作区内存中保留的更多连接。",
          "点击任意表格行打开该 Exchange 的完整连接时间线。",
          "如果列表显示读取失败，点击错误条中的“重试”；失败状态不等同于当前没有流量。",
        ],
      },
      {
        id: "capture-detail",
        title: "查看连接时间线",
        steps: [
          "详情按真实发生顺序追加连接建立、App 到 Proxy 收到、Proxy 到 Server 发送、Server 到 Proxy 收到、Proxy 到 App 发送、失败和关闭事件。",
          "HTTP Context 展示 Header 与 Body 文本；Socket Context 展示完整字节数据。",
          "Reader 已成功解析时，收到事件还会展示协议包固定生成的 Display；规则修改 Document 后不会回写或伪造旧 Display。",
          "失败事件保留阶段、方向、错误和当时可用的 Context，便于判断失败发生在读取、Frame、Decode、Rules、Encode 还是写入。",
          "关闭详情后会释放前端对完整报文的引用；内存仓储仍按容量策略保留连接证据。",
        ],
      },
      {
        id: "capture-refresh",
        title: "实时刷新与连接生命周期",
        steps: [
          "HTTP 或 Socket 产生新证据时，列表会在当前页面立即刷新，不需要切换导航或重新进入抓包页。",
          "同一 Socket 长连接会持续保留为同一个 Exchange；后续报文按顺序追加，不会覆盖此前证据。",
          "App 断开、Server 读取失败或业务 Pipeline 失败时，连接会追加关闭或失败事件并结束。",
          "HTTP 与 Socket 使用同一个实时事件订阅，任何协议的新连接都会刷新这张统一列表。",
        ],
      },
      {
        id: "capture-capacity",
        title: "理解内存容量与观测边界",
        steps: [
          "运行报文只保存在有界内存中，不写入数据库；重启应用后运行记录会清空。",
          "容量不足时优先淘汰最旧连接，并在页面显示已淘汰连接和已忽略事件数量。",
          "观测失败或观测数据淘汰不会影响交易；只有业务 Pipeline 失败才会影响连接。",
          "出现淘汰提示时，详情只代表当前仍保留的有序证据，不会补写或猜测已经丢失的事件。",
        ],
      },
      {
        id: "capture-clear",
        title: "清空运行记录",
        steps: [
          "点击“清空运行记录”会要求确认，然后统一清除当前工作区的 HTTP 与 Socket Exchange 内存记录。",
          "清空不会停止入口，也不会删除 Workspace、Listener、Rules 或协议包配置。",
          "观测失败或观测数据淘汰不会影响交易；只有业务 Pipeline 失败才会影响连接。",
          "清空后新建立或仍继续产生事件的连接会按照运行时事实重新出现在列表中。",
        ],
      },
    ],
  },
  "/rules": {
    title: "拦截规则",
    summary:
      "在 HTTP 与 Socket 之间切换，创建、编辑和管理拦截规则；HTTP 还提供小白可直接使用的故障预设。",
    recommendedFor:
      "重复执行报文修改、延迟、Mock、拒绝、断开、丢弃和截断等自动化场景，或用 HTTP 故障预设快速建立测试规则。",
    sections: [
      {
        id: "rules-list",
        title: "查看和管理规则列表",
        steps: [
          "列表按优先级升序执行；优先级相同的规则按创建顺序执行。",
          "查看名称、通道、阶段、匹配摘要、动作摘要、命中数和最后命中时间，确认规则是否作用于目标流量。",
          "使用每行开关启用或停用规则。停用后重新启用会重置该规则的第 N 次命中计数。",
          "点击某一行加载右侧编辑器；窄窗口会自动滚动到编辑区域。",
          "列表中的冲突提示表示规则可能被更高优先级的终止动作遮蔽，应检查执行顺序。",
        ],
      },
      {
        id: "rules-basic",
        title: "新建与基本信息",
        steps: [
          "点击“新建规则”，选择空白 HTTP、HTTP Body、Socket 报文规则或 HTTP 故障预设；所有规则都显示在同一列表中。",
          "在“基本”Tab 填写规则名称、说明和优先级。",
          "选择目标产品通道，再选择请求阶段或响应阶段；阶段会限制可添加的动作类型。",
          "决定保存后是否启用，以及是否“仅命中一次”。一次性规则命中后会自动停用。",
          "从抓包页“基于此请求新建规则”进入时，先检查预填的路径、通道和阶段，再补充动作。",
        ],
      },
      {
        id: "rules-conditions",
        title: "配置匹配条件",
        steps: [
          "打开“匹配条件”Tab，添加所需条件；所有条件共同参与匹配。",
          "可匹配终端、路径/请求类型、JSON 字段路径以及页面提供的其他字段。",
          "选择等于、包含、正则等操作符，并输入匹配值；保存时会严格校验正则和 JSON Path。",
          "配置“第 N 次命中”时，默认按终端 IP 与客户端证书指纹组合独立计数。",
          "修改匹配条件会重置命中计数；多终端测试时各终端不会共享默认命中次数。",
          "删除或切换异步草稿行后，等待状态会被清理，旧请求的迟到结果不会写入新的编辑行。",
        ],
      },
      {
        id: "rules-actions",
        title: "配置执行动作",
        steps: [
          "打开“执行动作”Tab，按实际执行顺序添加动作。",
          "修改 JSON 字段、替换 Body/Header、延迟和暂停可以组合，并会保存每一步轨迹。",
          "Mock、拒绝、断开、丢弃和截断是终止动作；命中后停止当前规则的后续动作和后续规则。",
          "请求阶段和响应阶段支持的动作不同；无效组合会被拒绝。",
          "输入响应 Header 时每行使用 name: value；输入字节时使用 0 到 255 的十进制逗号列表，提交后会解析并规范化。",
          "Shift-JIS Body、非法 JSON、错误长度和截断参数必须通过校验，页面不会静默沿用旧值。",
        ],
      },
      {
        id: "rules-save",
        title: "保存、复制、删除、导入和导出",
        steps: [
          "点击“保存规则”后等待完整校验字段、阶段、正则、JSON Path、Header、Shift-JIS 和终止动作顺序。",
          "字段错误会显示在相应控件下；异步草稿仍在等待或无效时保存按钮不可用。",
          "使用“复制”创建当前规则的独立副本，再修改名称、条件或动作后保存。",
          "删除规则需要二次确认，删除后无法恢复。",
          "“导入规则”会先校验整个文件；任何一条非法都会整体取消写入。",
          "“导出规则”不包含证书、密码、Payload 或机器专属路径，并使用系统文件对话框。",
        ],
      },
      {
        id: "rules-test",
        title: "验证规则是否生效",
        steps: [
          "启用规则并在“入口配置”启动目标入口，再在客户端应用发起满足条件的请求。",
          "到实时抓包查看“匹配规则”和规则轨迹，确认命中顺序、每个动作和最终结果。",
          "回到规则列表检查命中数和最后命中时间。",
          "终止动作未生效时检查是否选错通道/阶段、条件不匹配或被更高优先级规则提前终止。",
          "涉及客户端应用超时、自动取消或错误码的场景必须在真实设备验证，规则命中本身不等同于业务结果符合预期。",
        ],
      },
      {
        id: "faults-select",
        title: "HTTP 故障预设：选择模板",
        steps: [
          "在左侧表格查看模板的发生阶段、精确行为、影响端、默认参数和风险等级。",
          "点击某行或“配置”，右侧显示该模板的网络语义和可调整参数。",
          "区分请求阶段和响应阶段：请求阶段可能阻止 Server 收到请求；响应阶段通常已经让 Server 处理请求。",
          "高风险模板可能引起客户端应用超时、断开、自动取消或异常流程，使用前先确认测试环境。",
          "窄窗口中表格可横向滚动；选中模板后页面会自动把配置区域滚动到可见位置。",
        ],
      },
      {
        id: "faults-parameters",
        title: "填写模板参数",
        steps: [
          "先按模板填写 HTTP 状态、延迟、Body、长度差值、截断位置或其他专属参数。",
          "“终端过滤”留空表示所有终端；填写终端 ID 或 IP 可缩小作用范围。",
          "“路径与请求类型”用于限制目标接口或产品请求类型，避免故障影响无关流量。",
          "“第 N 次命中”决定满足条件后的第几次才触发；默认按终端 IP 与证书指纹分别计数。",
          "启用“一次性生效”后，规则首次命中会自动停用。",
          "“规则优先级”越小越早执行。若前面已有终止规则，当前模板可能无法命中。",
        ],
      },
      {
        id: "faults-enable",
        title: "从故障预设创建规则",
        steps: [
          "在“新建规则”中选择“从故障预设创建”，填写参数后点击“创建故障规则”。",
          "创建成功后会关闭模板窗口，并在常规规则列表中选中生成的普通规则，可继续添加复杂条件或组合动作。",
          "模板不会启动第二套执行引擎；最终执行顺序、命中计数和轨迹与普通规则完全一致。",
          "正在启动或停止 Proxy 时，故障写操作会被拒绝；等待生命周期操作完成后重试。",
          "参数错误会显示在对应字段，修正后重新提交。",
        ],
      },
      {
        id: "faults-semantics",
        title: "关键故障语义",
        steps: [
          "“不连接上游并断开”不会把请求发给 Server，直接关闭 App 连接。",
          "“发送上游后丢弃响应”会先把请求交给 Server，再不向客户端应用返回响应并断开；Server 可能已经完成处理。",
          "“Mock Shift-JIS JSON”绕过真实上游并返回 Shift-JIS 编码的模拟响应。",
          "“非法 JSON”返回可编码但语法非法的 JSON；“错误 Content-Length”和“截断响应”用于测试不完整报文处理。",
          "“请求前延迟/超时”和“响应延迟”发生阶段不同，应结合最近事件和分阶段耗时确认。",
          "任何模板都只保证产生定义的网络行为，不保证客户端应用一定显示某个固定 T02/T03/T04 或自动取消结果，必须实机记录。",
        ],
      },
    ],
  },
  ...systemPageHelpGuides,
};
