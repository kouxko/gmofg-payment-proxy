import type { WorkspacePath } from "@/features/shell/workspace-navigation";
import type { PageHelpGuide } from "./page-help-types";

/**
 * 协议包页面的只读操作说明。
 *
 * T17 只新增由 Rust 原生文件选择器驱动的两阶段 ZIP 导入；启停和删除仍不在
 * 当前页面交付范围。文案不能暗示 WebView 会读取本机路径、ZIP 字节或脚本源码。
 */
export const protocolPackageHelpGuide: Record<
  Extract<WorkspacePath, "/protocol-packages">,
  PageHelpGuide
> = {
  "/protocol-packages": {
    title: "协议包",
    summary:
      "按协议包 ID 汇总查看已安装的报文处理能力，并检查每个精确版本的校验、引用与字段结构。",
    recommendedFor:
      "配置 Socket 入口前确认目标协议版本可用、上下行能力符合预期，并了解规则可以引用的报文字段。",
    sections: [
      {
        id: "protocol-package-overview",
        title: "读取协议包概览",
        steps: [
          "列表按协议包 ID 分组，同一个协议包的多个版本只占一行。",
          "查看状态列，区分全部启用、全部停用和部分启用。",
          "查看版本数，确认应用已安装几个可供入口精确选择的版本。",
          "查看引用数和活动引用数，了解已有配置及运行中入口的使用情况。",
          "列表读取失败时使用重试；错误状态不能被当成尚未安装协议包。",
        ],
      },
      {
        id: "protocol-package-open-detail",
        title: "打开与关闭详情",
        steps: [
          "点击协议包行可打开详情，也可以让该行获得焦点后按 Enter 或空格。",
          "详情左侧列出该协议包的全部已安装版本，默认选择版本列表中的最新版本。",
          "按 Escape、点击关闭按钮或关闭遮罩都可以退出详情。",
          "关闭后焦点会回到刚才打开详情的协议包行，便于继续使用键盘浏览。",
        ],
      },
      {
        id: "protocol-package-import",
        title: "导入协议包 ZIP",
        steps: [
          "点击“导入协议包 ZIP”后，由桌面应用的原生文件选择器读取文件；页面不会接收本机路径或 ZIP 字节。",
          "选择文件后等待应用完成 ZIP、清单、字段结构、脚本模块和函数入口的全部校验；此阶段不会安装任何内容。",
          "校验通过后核对名称、精确 ID 与版本、宿主 API、能力、字段数和冲突结果，再明确确认安装或复用。",
          "校验失败时根据稳定错误码、包内逻辑文件、字段或行列修正协议包；失败不会留下部分安装记录。",
          "新安装版本默认停用，导入不会自动修改或重绑任何入口；后续需要在相应配置任务中显式选择精确版本。",
        ],
      },
      {
        id: "protocol-package-version",
        title: "核对精确版本",
        steps: [
          "在版本列表中选择目标版本，详情会按 package ID 与 SemVer 精确重新读取。",
          "核对名称、Package ID、SemVer、Host API 和安装时间，避免把相邻版本当成同一份实现。",
          "检查启用状态与最近一次持久化校验结果；校验异常的版本不应被用于新配置。",
          "切换版本后等待当前版本详情完成，不要用上一个版本的能力或字段结构判断当前版本。",
        ],
      },
      {
        id: "protocol-package-capabilities",
        title: "理解上下行能力",
        steps: [
          "Upstream 表示 App 到 Server 的方向，Downstream 表示 Server 到 App 的方向。",
          "分别检查两个方向是否具备报文边界、字段解析、报文重建和协议视图能力。",
          "协议视图是只读展示能力，用于把报文字段生成为应用可展示的内容。",
          "能力仅表示协议包声明并通过校验，不代表某个入口已绑定或正在运行。",
        ],
      },
      {
        id: "protocol-package-schema",
        title: "查看引用与字段结构",
        steps: [
          "使用者表只列出引用当前精确版本的工作区和入口，不与其他版本合并。",
          "运行状态用于判断该引用是否正在使用；停止状态仍然属于已保存引用。",
          "字段结构表中的字段名、标签和类型定义了解析后可用于规则的数据目录。",
          "编写动态规则时使用字段名作为变量依据，并按照 string、int、bool 或 blob 类型提供匹配值。",
          "字段结构没有字段时页面会明确提示，不能根据其他协议包或旧版本自行猜测字段。",
        ],
      },
    ],
  },
};
