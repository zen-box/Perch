---
name: 发布检查
description: 按检查清单过一遍再发布
---

发布前逐条确认：

1. `cargo fmt --check` 通过
2. `cargo clippy --all-targets` 没有代码警告
3. `cargo test` 全绿
4. 界面改动实际跑起来看过
