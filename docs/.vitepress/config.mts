import { defineConfig, type DefaultTheme } from "vitepress";

const repo = "https://github.com/Amazingkenneth/meetcue";

// Chinese has no spaces; split it into words for local search.
const segmenter = typeof Intl !== "undefined" && "Segmenter" in Intl ? new Intl.Segmenter("zh", { granularity: "word" }) : null;
function tokenize(text: string): string[] {
  if (!segmenter) return text.split(/[\s\p{P}]+/u).filter(Boolean);
  return [...segmenter.segment(text)].filter((s) => s.isWordLike).map((s) => s.segment);
}

function sidebarEn(): DefaultTheme.Sidebar {
  return [
    {
      text: "Get started",
      items: [
        { text: "Install", link: "/install" },
        { text: "First-time setup", link: "/setup" },
        { text: "Five-minute tour", link: "/quick-start" },
      ],
    },
    {
      text: "Guides",
      items: [
        { text: "Meeting mode", link: "/guide/meeting" },
        { text: "Mock interview", link: "/guide/mock-interview" },
        { text: "Practice mode", link: "/guide/practice" },
        { text: "Keyboard shortcuts", link: "/guide/shortcuts" },
        { text: "Latency probe", link: "/guide/latency" },
        { text: "Briefing material", link: "/guide/materials" },
        { text: "Providers and keys", link: "/guide/providers" },
      ],
    },
    {
      text: "Troubleshooting",
      items: [
        { text: "Overview", link: "/troubleshooting/" },
        { text: "Audio and devices", link: "/troubleshooting/audio" },
        { text: "Shortcuts and the panel", link: "/troubleshooting/panel" },
        { text: "Recognition and hint quality", link: "/troubleshooting/quality" },
        { text: "High latency", link: "/troubleshooting/latency" },
        { text: "Install and upgrade", link: "/troubleshooting/install" },
        { text: "Error codes", link: "/troubleshooting/error-codes" },
        { text: "Export diagnostics", link: "/troubleshooting/diagnostics" },
      ],
    },
    {
      text: "Reference",
      items: [
        { text: "Settings", link: "/reference/settings" },
        { text: "File formats", link: "/reference/file-formats" },
        { text: "Price reference", link: "/reference/pricing" },
        { text: "Privacy and data", link: "/reference/privacy" },
        { text: "FAQ", link: "/reference/faq" },
        { text: "Changelog", link: "/reference/changelog" },
        { text: "Hardware test checklist", link: "/test-checklist" },
      ],
    },
    {
      text: "Contribute",
      items: [
        { text: "Development setup", link: "/contribute/development" },
        { text: "Architecture", link: "/contribute/architecture" },
        { text: "Localization", link: "/contribute/localization" },
        { text: "Release process", link: "/contribute/release" },
      ],
    },
  ];
}

function sidebarZh(): DefaultTheme.Sidebar {
  return [
    {
      text: "开始",
      items: [
        { text: "安装", link: "/zh/install" },
        { text: "首次设置", link: "/zh/setup" },
        { text: "五分钟上手", link: "/zh/quick-start" },
      ],
    },
    {
      text: "指南",
      items: [
        { text: "会议模式", link: "/zh/guide/meeting" },
        { text: "模拟面试", link: "/zh/guide/mock-interview" },
        { text: "练习模式", link: "/zh/guide/practice" },
        { text: "快捷键", link: "/zh/guide/shortcuts" },
        { text: "延迟探测", link: "/zh/guide/latency" },
        { text: "会前资料", link: "/zh/guide/materials" },
        { text: "供应商配置", link: "/zh/guide/providers" },
      ],
    },
    {
      text: "排错",
      items: [
        { text: "排错总览", link: "/zh/troubleshooting/" },
        { text: "音频与设备", link: "/zh/troubleshooting/audio" },
        { text: "快捷键与悬浮窗", link: "/zh/troubleshooting/panel" },
        { text: "识别与提示质量", link: "/zh/troubleshooting/quality" },
        { text: "延迟偏高", link: "/zh/troubleshooting/latency" },
        { text: "安装与升级", link: "/zh/troubleshooting/install" },
        { text: "错误码参考", link: "/zh/troubleshooting/error-codes" },
        { text: "导出诊断信息", link: "/zh/troubleshooting/diagnostics" },
      ],
    },
    {
      text: "参考",
      items: [
        { text: "设置项", link: "/zh/reference/settings" },
        { text: "文件格式", link: "/zh/reference/file-formats" },
        { text: "价格参考", link: "/zh/reference/pricing" },
        { text: "隐私与数据", link: "/zh/reference/privacy" },
        { text: "常见问题", link: "/zh/reference/faq" },
        { text: "更新日志", link: "/zh/reference/changelog" },
        { text: "实机测试清单", link: "/zh/test-checklist" },
      ],
    },
    {
      text: "参与",
      items: [
        { text: "开发环境", link: "/zh/contribute/development" },
        { text: "架构", link: "/zh/contribute/architecture" },
        { text: "本地化指南", link: "/zh/contribute/localization" },
        { text: "发布流程", link: "/zh/contribute/release" },
      ],
    },
  ];
}

export default defineConfig({
  base: "/meetcue/",
  title: "MeetCue",
  cleanUrls: true,
  lastUpdated: true,
  head: [["link", { rel: "icon", type: "image/png", href: "/meetcue/logo.png" }]],
  locales: {
    root: {
      label: "English",
      lang: "en",
      description: "Live talking points for meetings and interviews, and a practice mode to get used to the pace.",
      themeConfig: {
        nav: [
          { text: "Guide", link: "/guide/meeting" },
          { text: "Troubleshooting", link: "/troubleshooting/" },
          { text: "Reference", link: "/reference/settings" },
        ],
        sidebar: sidebarEn(),
        editLink: { pattern: `${repo}/edit/main/docs/:path`, text: "Edit this page on GitHub" },
        docFooter: { prev: "Previous", next: "Next" },
        outline: { label: "On this page", level: [2, 3] },
        lastUpdated: { text: "Last updated" },
      },
    },
    zh: {
      label: "简体中文",
      lang: "zh-CN",
      link: "/zh/",
      description: "会议与面试的实时提词助手，并提供模拟练习帮你适应节奏。",
      themeConfig: {
        nav: [
          { text: "指南", link: "/zh/guide/meeting" },
          { text: "排错", link: "/zh/troubleshooting/" },
          { text: "参考", link: "/zh/reference/settings" },
        ],
        sidebar: sidebarZh(),
        editLink: { pattern: `${repo}/edit/main/docs/:path`, text: "在 GitHub 上编辑此页" },
        docFooter: { prev: "上一页", next: "下一页" },
        outline: { label: "本页目录", level: [2, 3] },
        lastUpdated: { text: "最后更新" },
        returnToTopLabel: "回到顶部",
        sidebarMenuLabel: "菜单",
        darkModeSwitchLabel: "外观",
        langMenuLabel: "语言",
      },
    },
  },
  themeConfig: {
    logo: "/logo.png",
    socialLinks: [{ icon: "github", link: repo }],
    search: {
      provider: "local",
      options: {
        miniSearch: { options: { tokenize }, searchOptions: { fuzzy: 0.2, prefix: true, tokenize } },
        locales: {
          zh: {
            translations: {
              button: { buttonText: "搜索", buttonAriaLabel: "搜索" },
              modal: {
                noResultsText: "没有找到结果",
                resetButtonTitle: "清除",
                footer: { selectText: "选择", navigateText: "切换", closeText: "关闭" },
              },
            },
          },
        },
      },
    },
  },
});
