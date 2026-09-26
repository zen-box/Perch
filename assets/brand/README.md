# 品牌图标

这里放模型厂商和渠道的 SVG 图标，构建时会自动嵌入程序（见 `build.rs`）。

- 来源：[LobeHub Icons](https://icons.lobehub.com) 的 npm 包 `@lobehub/icons-static-svg`，MIT 协议。
- 文件名与包里的 `icons/` 目录一致，例如 `claude.svg`、`gemini-color.svg`。
- 需要哪些文件由 `src/brand.rs` 里的品牌表决定：
  - 单色头像用 `<名字>.svg`；
  - 彩色头像（Gemini、豆包等）用 `<名字>-color.svg`。
- 缺少的图标会显示成品牌色加首字母，不影响使用。

放入或删除文件后重新 `cargo build` 即可生效。
