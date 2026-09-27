# P3-2 端到端实测环境

P3-2（Agent 循环）当初是用这套东西实测的，不是单测——单测盖不到「界面卡不卡、
授权卡片长什么样、请求体到底发了什么」。搬出 `target/` 是为了不被 `cargo clean` 清掉。

设计上的两条硬约束（跟 `AGENTS.md` 的测试约定一致）：

- **不碰真实数据**：程序用 `APPDATA` 指到本目录，读写的都是这里的副本；
- **不访问外网**：模型接口由 `mock.py` 在 `127.0.0.1` 上顶替。

## 文件

| 文件 | 干什么 |
| --- | --- |
| `mock.py` | 模拟 OpenAI Chat Completions 的**流式**接口。每个请求体追加写进 `requests.jsonl`；**按 OpenAI 的规则校验工具调用配对**，不合规就回 400（报错文案抄的 OpenAI）。回什么由最后一条用户消息里的关键词决定，见文件头注释 |
| `win.ps1` | 用 `PostMessage` 操作测试窗口：改尺寸、截图（`PrintWindow`，被挡住也能截）、点击、悬停、滚轮、打字、回车。**不抢焦点、不动真实鼠标键盘**，所以能一边跑一边干别的 |
| `t.sh` | 薄封装，`source` 之后用 `send` / `shot` / `nreq` / `reqs` 四个函数 |
| `findbtn.py` | 不依赖 Pillow 的 PNG 像素扫描器。`win.ps1` 的点击坐标必须是截图里的物理像素，肉眼估误差太大，所以靠扫像素找按钮中心 |
| `cwd/` | 隔离的工作目录（一个带 `.env` 的假项目），给工具调用当靶子。`.env` 是特意放的——用来验证敏感路径会不会被拦 |
| `mock-config.json` | 一份现成的 `perch-config.json`：渠道指向 mock 服务、开了本地工具、超时 600 秒。**故意不叫 `perch-config.json`**——仓库根 `.gitignore` 有一条 `perch-*.json`（防真实配置带密钥被提交），改名是为了不跟那条规则打架 |
| `shots/` | 实测留下的截图。`A`~`H` 是 P3-2 主流程，`v1`~`v8` 是收尾（耗时/退出码、超时可配）那批 |

## 怎么跑

需要 Windows + PowerShell + Python 3（`mock.py` / `findbtn.py` 只用标准库）。
`t.sh` 的 `shot` 会调 Pillow 存一份缩小版，所以要装 Pillow；不装就把那行去掉。

1. 编译并把 exe 改名。`win.ps1` 是按**进程名** `perch-p3` 找窗口的，
   所以文件名必须是 `perch-p3.exe`。放在 `target/` 里（那儿已被 gitignore，不会弄脏仓库）：

   ```bash
   cargo build --bin perch
   cp target/debug/perch.exe target/perch-p3.exe
   ```

2. 起模拟服务（端口默认 18766，也可以 `python mock.py 18766`）：

   ```bash
   python tools/p3-test/mock.py &
   ```

3. 用隔离的 `APPDATA` 启动。先把 `mock-config.json` 摆成程序认的路径，省得手填渠道：

   ```bash
   mkdir -p tools/p3-test/appdata/Perch
   cp tools/p3-test/mock-config.json tools/p3-test/appdata/Perch/perch-config.json
   APPDATA="$PWD/tools/p3-test/appdata" ./target/perch-p3.exe &
   ```

4. 定窗口尺寸，再开 `t.sh` 的函数：

   ```bash
   source tools/p3-test/t.sh
   W -Action size -W 1774 -H 1172     # 尺寸定了坐标才有意义
   send "LIST"                        # 触发一次 list_directory 调用
   shot A-list
   reqs 0                             # 看模拟服务收到的请求
   ```

## 坑

- **`t.sh` 一 `source` 就 `cd` 到自己所在目录**（`tools/p3-test/`），
  之后相对路径都是相对那儿的。要回仓库根得自己 `cd`。
- **`t.sh` 里的坐标是写死的**（输入框 `1080,1188`）。换窗口尺寸就得重算，
  用 `findbtn.py` 扫一张截图重新定位。
- `mock.py` 的 `requests.jsonl` 写在它**自己旁边**（不是当前目录），跑完记得看一眼。
- `mock.py` 有状态：`call_N` 的计数器在进程里，重启服务会从头开始编号。
- 关键词场景（`LIST` / `RUN` / `SLEEP` / `PAR` / `LOOP` / `SLOW`）见 `mock.py` 文件头，
  改场景直接改那里。

## 实测结论记在哪

`TODO.md` 第三节（P3-2 修复的 7 个问题）和第四节（工具后台执行）是这轮实测的结论，
现象、原因、改法都在那儿。本文只讲怎么把环境跑起来。
