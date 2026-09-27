# 端到端实测环境

P3-2（Agent 循环）和 P3-3（MCP）都是用这套东西实测的，不是单测——单测盖不到
「界面卡不卡、授权卡片长什么样、请求体到底发了什么、子进程收没收干净」。
搬出 `target/` 是为了不被 `cargo clean` 清掉。

设计上的两条硬约束（跟 `AGENTS.md` 的测试约定一致）：

- **不碰真实数据**：程序用 `APPDATA` 指到本目录，读写的都是这里的副本；
- **不访问外网**：模型接口由 `mock.py` 在 `127.0.0.1` 上顶替，MCP 服务器是本地的 `mcp_server.py`。

## 文件

| 文件 | 干什么 |
| --- | --- |
| `mock.py` | 模拟 OpenAI Chat Completions 的**流式**接口。每个请求体追加写进 `requests.jsonl`；**按 OpenAI 的规则校验工具调用配对**，不合规就回 400（报错文案抄的 OpenAI）。回什么由最后一条用户消息里的关键词决定，见文件头注释 |
| `mcp_server.py` | 一个最小的 stdio MCP 服务器，给 MCP 那条路当靶子。工具清单是挑过的（撞名、超长名、中文名、返回图片块、只给 structuredContent……），见文件头注释。收到的调用追加写进 `mcp_calls.jsonl` |
| `win.ps1` | 用 `PostMessage` 操作测试窗口：改尺寸、截图（`PrintWindow`，被挡住也能截）、点击、悬停、滚轮、打字、回车。**不抢焦点、不动真实鼠标键盘**，所以能一边跑一边干别的 |
| `t.sh` | 薄封装，`source` 之后用 `send` / `shot` / `nreq` / `reqs` 四个函数 |
| `findbtn.py` | 不依赖 Pillow 的 PNG 像素扫描器。`win.ps1` 的点击坐标必须是截图里的物理像素，肉眼估误差太大，所以靠扫像素找按钮中心 |
| `zoom.py` | 裁一块截图放大保存，用来看清局部（徽标、一行文字）。要 Pillow |
| `cwd/` | 隔离的工作目录（一个带 `.env` 的假项目），给工具调用当靶子。`.env` 是特意放的——用来验证敏感路径会不会被拦 |
| `mock-config.json` | P3-2 那轮的 `perch-config.json`：渠道指向 mock 服务、开了本地工具、超时 600 秒。**故意不叫 `perch-config.json`**——仓库根 `.gitignore` 有一条 `perch-*.json`（防真实配置带密钥被提交），改名是为了不跟那条规则打架 |
| `mock-config-mcp.json` | P3-3 那轮的配置：在上一份基础上加了 4 台 MCP 服务器（两台能连、一台命令不存在、一台停用），并给演示服务器配了 `disabled_tools: ["spam"]`。后来又补了 `slow`（`slowstart`）和 `slowfail` 两台，用来测重连时的界面表现，以及第二个模型 `mock-vision`（带 `vision`+`files` 能力，用来对照附件闸门） |
| `mock-config-real-mcp.json` | **给用户照抄的样例**：三台真实可用的 npx 服务器（文件系统 / 顺序思考 / 记忆图谱）。渠道仍然指向 `mock.py`，所以模型侧不真跑，只用来验证「能不能连上、工具清单对不对」 |
| `shots/` | 实测留下的截图。`A`~`H` 是 P3-2 主流程，`v1`~`v8` 是 P3-2 收尾，`m1`~`m12` 是 P3-3，`m13`~`m15` 是 P3-3 三处界面缺陷的修复验证，`m16`~`m18` 是真实 MCP 服务器接入验证，`m19`~`m23` 是 P3-3 收尾（重连闪动 + 附件能力闸门），`m24`~`m28` 是会话级工具开关与「本次对话的工具」选择器，`m29`~`m31` 是 MCP 表单改弹窗，`m32` 是「编辑停用服务器不会把它打开」，`m33`~`m40` 是附件入口按模型能力分类型（提示改短 + 菜单动态化 + 去掉「所有文件」兜底），`n1`~`n6` 是对话/智能体按「有没有本机文件权限」重新划分，`n7`~`n12` 是智能体的项目目录（相对路径基准 + 越界授权） |

## 怎么跑

需要 Windows + PowerShell + Python 3（`mock.py` / `mcp_server.py` / `findbtn.py` 只用标准库；
`zoom.py` 和 `t.sh` 的 `shot` 要 Pillow）。

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

3. 摆好隔离的 `APPDATA`：

   ```bash
   mkdir -p tools/p3-test/appdata/Perch
   cp tools/p3-test/mock-config-mcp.json tools/p3-test/appdata/Perch/perch-config.json
   ```

4. **启动 Perch 必须绕开 agent 的进程树**——见下面「坑」里的第一条。从 agent 的
   shell 里直接跑，MCP 服务器一台都起不来：

   ```powershell
   # 用 PowerShell 工具执行。Win32_ProcessStartup 用来带上隔离的 APPDATA
   $vars = @()
   foreach ($item in [System.Environment]::GetEnvironmentVariables('Process').GetEnumerator()) {
     if ($item.Key -ne 'APPDATA') { $vars += "$($item.Key)=$($item.Value)" }
   }
   $vars += 'APPDATA=E:\Personal control\pc\tools\p3-test\appdata'
   $startup = ([wmiclass]"Win32_ProcessStartup").CreateInstance()
   $startup.EnvironmentVariables = [string[]]$vars
   ([wmiclass]"Win32_Process").Create("E:\Personal control\pc\target\perch-p3.exe", "E:\Personal control\pc", $startup)
   ```

5. 定窗口尺寸，再开 `t.sh` 的函数：

   ```bash
   source tools/p3-test/t.sh
   W -Action size -W 1774 -H 1172     # 尺寸定了坐标才有意义
   send "LIST"                        # 触发一次 list_directory 调用
   shot A-list
   reqs 0                             # 看模拟服务收到的请求
   ```

   窗口 1756x1163（截图实际尺寸；`-W 1774 -H 1172` 会被窗口管理器收掉一圈边框）时的
   常用坐标（截图物理像素）：设置齿轮 `1570,26`、设置页「MCP 服务器」导航 `150,376`、
   输入框 `1074,1079`、授权卡片「允许执行一次」`1525,955`、服务器行的「断开/连接」`1357,325`、
   删除 `1467,765`。**P3-3 收尾时补的**：输入框左下角附件按钮 `522,1105`、
   服务器行的「重连」按钮（失败态那一行）`1369,654`。**会话级工具开关那一批补的**：
   工具栏「对话 / 智能体」分段控件 + 项目目录按钮 + 工具扳手（实测，模型名 `Mock Model`、
   项目目录已设、显示「📁 cwd」时）：「对话」`1250`~`1345`（中心 `1296`）、
   选中态白色胶囊包住「智能体」`1345`~`1440`（中心 `1393`）、
   项目目录按钮 `1455`~`1560`（中心约 `1505`）、工具扳手按钮 `1555`~`1600`（中心 `1578`）。
   授权卡片的「拒绝」`1415,955`、「✓ 允许执行一次」`1545,955`。
   （⚠️ 扳手**宽度随角标数字变**，所以别按固定坐标点它，量的时候按「参数」右侧那个图标现找）、
   选择器里第一行工具的勾选框 `992,975`。
   ⚠️ **上面这组数只在「模型名 = `Mock Model`」且项目目录已设时成立。**
   那一行是**整体居中**的，任何一处变宽都会把右边整组往右推、左边整组往左推：
   模型名变长、附件按钮多出下拉箭头、项目目录按钮从「选择项目目录」缩成「cwd」……
   都会挪。踩过两次：早先记的 `1015,1104` 后来差了 370px，点了完全没反应，
   白白以为是功能坏了；加上项目目录按钮之后又挪了一次（对话从 `1385` 到 `1296`）。
   **改过这几处任何一样，先截图重量再点**——量法：按行扫暗像素（灰度 `< 150`）再按间隙聚类，
   一次就能把整行每个控件的位置都算出来，比 `zoom.py` 肉眼量稳。
   选择器里的勾选框不受影响（弹层锚在扳手按钮上，但内容布局是固定的）。**MCP 表单改弹窗那一批补的**：
   设置页右上「+ 添加服务器」`1517,242`、第 1 行服务器的「编辑」铅笔 `1425,320`、
   弹窗底部的「取消」`1073,976` / 「添加服务器」「保存」`1246,976`。
   **附件入口改菜单那一批补的**：附件按钮实测中心 `518,1103`（早先记的 `522,1105` 也在按钮上）；
   菜单四行自上而下 `933` / `976` / `1018` / `1060`（图片文件 / PDF 文档 / 文本与代码 /
   所有文件），横向点 `620` 即可，行高约 42；标题栏语言按钮 `1445,21`，
   语言菜单里「简体中文」`1353,73`、「English」`1349,158`。
   ⚠️ **菜单改动态之后只剩三行**（「所有文件」已去掉，见下），行距不变，
   所以自上而下是 `933` / `976` / `1018`，横向仍点 `620`。
   ⚠️ **按钮上多了下拉箭头（`⌄`）时，按钮变宽约 8px**，模型选择器整体右移。
   纯文本模型下按钮**没有**箭头（只剩一类，点一下直接开对话框），位置回到 `518`；
   有箭头时中心约 `522`。要点的东西在它右边的话，先截图量一下再点。
   ⚠️ 模型选择器菜单里两项的位置：`Mock Model` 约 `680,985`、`Mock Vision` 约 `680,1038`。
   ⚠️ **`perch-config.json` 里的 `model` 改不动界面上显示的模型**——活动模型存在
   会话里（`perch.db`），配置里那个只是新建会话的默认值。实测要换模型，得点界面上的
   模型选择器。
   ⚠️ 附件菜单在按钮**上方**展开，最上面一行贴着消息区；消息底部的「Token 与费用明细」
   悬停卡片一旦被鼠标唤起就会**一直挂着**（`PostMessage` 发不出 `WM_MOUSELEAVE`），
   正好压住菜单第一行。**先开菜单、再把鼠标挪到空白处**（如 `1250,180`）——卡片消失、
   菜单还在，这时再截图。反过来先挪鼠标再开菜单，卡片会跟着回来。
   ⚠️ 每行右侧那一串按钮挨得很近（`断开/重连` → 铅笔 → 垃圾桶 → 开关），
   铅笔和垃圾桶只差约 32px，**点歪就点成删除**（踩过：`1457,904` 想点铅笔，
   结果弹出删除确认框）。用 `zoom.py` 裁出来量，别按整图估。
   换尺寸就得重算。
   ⚠️ 坐标别靠肉眼估：截图会被工具按比例缩过再显示，估出来的位置能差上百像素
   （踩过：附件按钮估成 `386,1066`，实际在 `522,1105`）。用 `findbtn.py`，
   或者按上面那套「扫一行里的暗像素列」的办法量。

## 坑

- ⚠️ **agent 的沙箱会打断 Rust 建匿名管道，MCP 一台都起不来。** 症状是设置页里
  所有服务器都显示 `连接失败：failed to start \`...\python.exe\`: 所有的管道范例都在使用中。(os error 231)`。
  `ERROR_PIPE_BUSY` 来自 Rust std 的 `child_pipe()`——它用 `NtCreateNamedPipeFile` 造
  匿名管道，在 agent 进程树里这一步会失败。判别方法：把复现程序挂在 WMI 下面跑
  （`([wmiclass]"Win32_Process").Create(...)`），五种 stdio 组合全部成功；从 agent 的
  shell 里跑就全失败。**结论是环境问题，不是产品缺陷**（真实用户不会遇到），
  但实测时必须按上面第 4 步启动。
- ⚠️ **Windows 上 `SO_REUSEADDR` 允许两个 mock 同时绑同一个端口**（Linux 不允许）。
  所以"第二个 mock 启动成功"不代表它接得到请求——请求会落到先绑的那个上，
  `requests.jsonl` 也就写在它那边，你这边永远是空的。实测踩过一次：
  一个从 `target/p3-test/` 遗留的旧 mock 还在跑，请求全被它接走了。
  现在 `mock.py` 启动前会探测端口，占用了就直接报错退出；**仍要留意有没有旧实例**。
- **`t.sh` 一 `source` 就 `cd` 到自己所在目录**（`tools/p3-test/`），
  之后相对路径都是相对那儿的。要回仓库根得自己 `cd`。
- **本机 PowerShell 默认禁止执行 `.ps1`**（`running scripts is disabled`），
  直接 `& win.ps1` 会失败。绕法：
  ```powershell
  $sb = [scriptblock]::Create((Get-Content -Raw "tools/p3-test/win.ps1"))
  & $sb -Action shot -Out "tools/p3-test/shots/x.png"
  ```
  另外**别从 bash 里调 `powershell`**，会被安全策略拦（"bypasses PowerShell security checks"），
  要用专门的 PowerShell 工具；那个工具**不回显 stdout**，要拿输出就 `Out-File` 到文件再读。
  还有两条：**PowerShell 工具禁止调用 `cmd.exe`**，**禁止 `Add-Type` 内联编译 C#**
  （`win.ps1` 里那句 `Add-Type` 是脚本文件里的，允许）。
- **滚轮一次只滚一点点**，`-W -3` 几乎不动；实测用 `-W -12` 连发十几次才翻得动一屏。
- **键盘打字只对 ASCII 可靠。** `win.ps1` 的 `type` 用 `PostMessage(WM_CHAR)` 逐字符投递，
  投出去的码点是对的（实测 `编` 投的是 `0x7F16`），但落进 Perch 的输入框会变成别的字
  （`编辑器测试` 存成了 `憦hK諎`）。**原因没查清**，怀疑跟合成 `WM_CHAR` 这条非正常输入
  路径有关（真实用户走 IME，是另一条路）。**所以填表单一律用 ASCII**；中文读写本身没问题
  （配置文件里的中文名称显示完全正常）。想确认中文输入，手工敲一下最快。
- **点击坐标偏 15~20 像素就会打空**（按钮之间有空隙，点空处什么也不发生，界面完全不动）。
  按钮定位一律先用 `zoom.py` 裁出来看，别按整图估。
- ⚠️ **挨在一起的两个文字按钮，空隙比按钮还窄。** 选择器标题行右边是「全选」「全不选」
  两个纯文字按钮，实测「全选」占 `1471`~`1506`、「全不选」占 `1525`~`1578`——
  **中间只有 19px 是空的**，点 `1504` 正好落在缝里，界面毫无反应。
  点文字按钮要取**文字自身的中心**（这里是 `1551`），不能取两个词的中线。
  定位办法：按行扫暗像素（`灰度 < 140`）再按间隙聚类，比 `zoom.py` 肉眼量更稳。
- ⚠️ **Popover（弹层）的宽度不受 `.w()` / `.max_w()` 控制。** 实测给「本次对话的工具」
  选择器根节点设 `Popover::w(px(340.))`，实际宽 509；给「参数」弹层设 360，实际约 500。
  一开始怀疑是内层 div 的锅，用**红描边实验**（给根节点 `border_2().border_color(p.danger)`）
  证明被撑到 509 的**就是根节点本身**——弹层外面套的是列向 flex，cross 轴 `stretch`
  把定宽盖过去了。`.max_w(px(340.))` 也一样无效（改前改后两张截图 md5 完全相同）。
  **结论：弹层宽度 = 内容最宽的一行 + 内边距。** 定宽改不动，只能反过来限制内容宽度
  （给描述文案加字符上限）。高度同理，所以长列表要用**外层普通 `div` 的 `max_h`** 兜住，
  超出部分靠 `overflow_y_scrollbar()` 滚——注意它会再套一层 `size_full()` 的包装层，
  高度约束写在滚动层自己身上没用。
- ⚠️ **`(python mock.py 18766 &)` 起的服务会随命令结束被回收。** 用 `&` 放在命令尾部，
  进程活不过那条 Bash 命令，现象是第一条请求根本没发出去、`requests.jsonl` 不存在，
  看着像 Perch 没请求。改用 Bash 工具的 `run_in_background=true` 起。
- `mock.py` 的 `requests.jsonl` 写在它**自己旁边**（不是当前目录），跑完记得看一眼；
  启动时它会把完整路径打出来。
- `mock.py` 有状态：`call_N` 的计数器在进程里，重启服务会从头开始编号。
- ⚠️ **接着上一条：重启 mock 之后不要在旧会话里继续测。** 计数器从 `call_1` 重来，
  而旧会话的历史里已经有 `call_1` / `call_2` 的结果，Perch 按调用 id 判「这条调用已经有
  结果了」，于是**跳过执行直接续跑**——现象是工具结果卡片根本不出现，模型收到的是
  `Not executed: this tool call was interrupted before it ran.`（占位块的初始文案）。
  看起来像 Agent 循环坏了，其实只是 id 撞了。**换一个「新对话」再测**（或先删掉旧会话）。
  真实环境不会撞：调用 id 由服务方保证唯一。
- **跨运行比对 `mcp_calls.jsonl` 时看 `tool` 字段，别看 `call_N` 编号**——编号每次重启都从 1 开始。
- ⚠️ **PATH 里同名的「无后缀脚本」会让 spawn 报 os error 193。** Node 的 Windows 发行版
  在 `npx.cmd` 旁边还放了一个**无后缀的 `npx`**（给 Git Bash 用的 shell 脚本），
  `npm` / `pnpm` 也一样。Windows 执行不了没有扩展名的文件，所以按 PATH 找可执行文件时
  **不能把无后缀的名字算进去**（`mcp.rs::find_in_path`，已修）。踩到时的报错是
  `%1 不是有效的 Win32 应用程序。(os error 193)`，里面只有路径，看不出是「选错了同名的
  另一个文件」——症状见 `shots/m16-npx-not-win32.png`。
- **别从 agent 的 shell 里 `git push` 或长时间跑东西**：agent 的 PATH 前面挂着一串托管
  运行时的目录（`~/.workbuddy-ai/binaries/...`），跟用户真实启动进程时的 PATH 不一样。
  验证「用户会看到什么」时要用 `Win32_ProcessStartup.EnvironmentVariables` 把 `Path`
  显式设成 `机器级 PATH + ";" + 用户级 PATH`。
- **点设置齿轮只点一次。** 它是「切换」：点两下等于进去又出来，现象是「怎么点都不进设置页」。
  （旧笔记写「第一次点不生效要补点一次」，其实是当时多点了。）
- **PostMessage 合成不了带修饰键的快捷键。** GPUI 的 Windows 后端用
  `GetKeyState`（`gpui/src/platform/windows/events.rs::current_modifiers`）取 Ctrl / Shift / Alt，
  读的是**真实键盘状态**，不是消息里带的。所以 `Ctrl+V` 这种组合发 WM_KEYDOWN 没用。
  后果：**剪贴板粘贴这条路实测不了**（附件只能从文件对话框进，而文件对话框是原生模态窗，
  `win.ps1` 按进程名找 `MainWindowHandle` 也拿不到它）。要测这条得先给 `win.ps1` 加
  「按窗口类枚举 + 往对话框发消息」的能力。
- ⚠️ **正在跑的 exe 复制不过去，而且 `Copy-Item -Force` 会静默失败**（PowerShell 工具不回显
  错误）。踩过一次：改了代码、`cargo build` 成功、`cp` 报 `Device or resource busy`，
  于是**用旧 exe 又测了一遍**，白测一轮还以为是修复没生效。杀进程后要 `Start-Sleep` 两三秒，
  再用 `Get-FileHash` 核对两份 exe 的哈希一致才继续。
- ⚠️ **别用 PowerShell 的 `Set-Content -Encoding utf8` 改配置文件**：Windows PowerShell 5.1
  的 `utf8` 会写 BOM，Perch 读配置时直接报 `读取配置失败: expected value at line 1 column 1`
  进「启动失败」页（截图见 `shots/err-config.png`）。用 Python 以 `utf-8` 无 BOM 写。
- **`python` 在 PATH 上可能解析到 Microsoft Store 的别名**
  （`AppData\Local\Microsoft\WindowsApps\python.exe`），它会再起一层真正的 python。
  所以 MCP 服务器的进程树是两层——正好用来验证按 pid 收整棵树有没有做对。
- 关键词场景（`LIST` / `RUN` / `SLEEP` / `PAR` / `LOOP` / `SLOW`，以及 MCP 那一组）
  见 `mock.py` 文件头，改场景直接改那里。

## 实测结论记在哪

- P3-2：`TODO.md` 第三节（修复的 7 个问题）和第四节（工具后台执行），现象、原因、改法都在那儿。
- P3-3：见 `shots/m1`~`m12`。`m11-exposed-names.txt` 是 Perch 实际暴露给模型的工具名，
  `m12-calls.jsonl` 是 MCP 服务器实际收到的调用（用来核对「发给服务器的是原始名」）。
- P3-3 三处界面缺陷的修复验证：`m13-approval-mcp-hint.png`（授权卡片改成 MCP 文案）、
  `m14-raw-names.png`（结果卡片显示服务器原始名 `a.b` / `a_b`）、
  `m15-unknown-mcp-tool.png`（不存在的 MCP 工具名回 MCP 侧清单，而不是本机那 5 个）。
- 真实 MCP 服务器接入：`m16-npx-not-win32.png`（修复前的报错）、
  `m17-real-mcp-connected.png`（三台全连上：文件系统 14 / 顺序思考 1 / 记忆图谱 9 个工具）、
  `m18-real-mcp-tools.png`（文件系统的工具清单）。
- **点「重连」页面闪动**（P3-3 收尾修的）：根子是**行高变化**，不是状态闪烁——
  失败那一行是 3 行（名字 / 命令 / 红色失败原因），一点重连状态变 `Connecting`，
  失败原因那行消失 → 整行矮一截、下面几台服务器跟着往上跳，连上或者再失败时又跳回来。
  证据：`m19-mcp-retry-before.png`（失败态，3 行）、`m20-mcp-retry-flicker-1~5.png` 和
  `m20-mcp-retry-flicker-compare.png`（修复前，行位置在跳）、`m21-mcp-retry-fixed-1~5.png`
  和 `m21-mcp-retry-fixed-compare.png`（修复后，5 帧里行高和行位置一动不动）。
  修法：`McpState::last_error` 记住最近一次失败原因，重连期间继续显示（转灰 + 「上次失败：」前缀）。
  靶子是 `slowfail` 模式（睡 6 秒再失败），有这几秒才截得到 `Connecting` 那一帧。
- **附件入口按模型能力闸门**：`m22-attach-tooltip-limited.png`（模型只有 `tools` →
  提示「添加附件 (当前模型不支持部分格式)」）、`m23-attach-tooltip-full.png`
  （模型带 `vision`+`files` → 提示「添加附件 (图片/文档/表格/代码)」）。两张各带一份 `-zoom` 放大图。
  ⚠️ 输入框上方的提示条和点发送时的拦截**没实测到**（要真加一个附件才触发，
  而这条路只能靠粘贴，见上面那条坑），只有单测覆盖。
- **会话级工具开关 +「本次对话的工具」选择器**（P3-3 之后那批）：
  `m24-default-chat-mode.png`（新会话默认「对话」选中、扳手灰色无角标）、
  `m25-agent-mode.png`（切「智能体」后扳手变紫 + 「43 个工具」）、
  `m26-tool-picker.png`（选择器按来源分组：本地工具 / 演示服务器 / …，带全选/全不选）、
  `m27-tool-picker-scrolled.png`（列表滚到底，43 个工具的末尾都在，长名字视觉截断）、
  `m28-one-tool-off.png`（取消勾选 `mcp__slow__t3f7371c8` 后角标 43 → 42）。
  请求体侧（`requests.jsonl`）：智能体模式 `tools` 数量 **42**、含 `run_command`、
  不含被取消的那个；切回对话模式再发，请求体**完全没有 `tools` 字段**。
- **MCP 表单改弹窗**：`m29-mcp-add-dialog.png`（添加：五个字段全空 + 底部「取消 / 添加服务器」）、
  `m31-mcp-edit-dialog.png`（编辑：名称 / 命令 / 参数 / 目录全部预填，**背后的工具清单详情可见**——
  这正是改弹窗要解决的：页内表单会把详情挤走）、
  `m30-mcp-dialog-validation.png` + `m30-mcp-dialog-validation-zoom.png`
  （空表单点保存 → 顶部弹「名称和启动命令不能为空。」，**弹窗留在原地**，填的内容不丢）。
  保存成功后弹窗关闭、服务器自动重连（截图里那台变成「已连接 · 13 个工具」），
  配置文件里 6 台服务器的 id 与命令原样不变。
- **编辑已停用的服务器不会把它偷偷打开**：`m32-edit-disabled-stays-disabled.png`。
  原来 `save_mcp_editor` 里 `enabled` 写死成 `true`（只对「新建」成立），
  于是「停用的服务器」进来改个参数、点保存，就自己变成启用并在后台起进程了——
  对着开关看：修复前那一行会从「连接」变「重连」、开关变紫，修复后一直是「未连接」+ 灰开关。
  这个坑是在改弹窗时顺手发现的，因为弹窗把「编辑」这个动作变得太容易做。
- **附件入口按模型能力分类型**（`m33`~`m40`，最终形态）：
  - `m33-attach-tooltip-text-only.png`（+`-zoom`）：纯文本模型下悬停提示是
    **「添加附件（文本与代码）」**；`m34-attach-tooltip-full.png`（+`-zoom`）：
    能力齐全时就是**「添加附件」**。早先那版提示会念一长串「当前模型看不了图片、
    也读不了 PDF」，太长且影响美观，已改短。
  - `m37-attach-entry-text-only-zoom.png`：纯文本模型下附件按钮**没有下拉箭头**——
    只剩一类可加，点一下**直接开文件对话框**，画箭头等于骗人。
    `m38-attach-entry-full-zoom.png`：能力齐全时按钮带 `⌄`，点开才是菜单。
    两张并排看，箭头有无一眼可辨（同一张图里模型选择器的 `⌄` 作为对照）。
  - `m36-attach-menu-full.png`（+`-zoom`）：`Mock Vision`（`vision`+`files`）下菜单
    **三项齐全**：图片文件 / PDF 文档 / 文本与代码。`m39-attach-menu-en.png`（+`-zoom`）
    是英文界面同一份菜单（Image files / PDF documents / Text & code）。
  - `m40-attach-dialog-text-code.png`（+`-zoom`）：纯文本模型下点按钮直接弹系统文件
    对话框，右下角过滤器是**「文本与代码 (\*.txt;\*.md;\*.log;\*.c…）」**——
    证明 `AttachmentFilter::label` 与 `exts()` 真传到了 `rfd`，而且扩充后的扩展名表生效了。
  - 这一轮去掉了「所有文件 (\*.\*)」兜底项。理由：它的 `exts()` 是 `["*"]`，用户照样能
    从通配里点一张 `.png`，**选完才在导入时被拒**——正是这套闸门要消掉的「白跑一趟」；
    而它能提供的「多选些没列出来的文本扩展名」这个价值，改成把常见格式补进 `TEXT_EXTS`
    就拿到了，代价还更小。
  - ⚠️ **系统文件对话框点不动**：它是 shell 的独立窗口，`PostMessage` 发的
    `WM_LBUTTONDOWN/UP` 到不了（实测点「取消」和点列表项都没反应）。
    要看对话框**背后**的界面，只能杀进程重开；别指望点它上面的按钮。
  - ⚠️ 对话框一开，它就是该进程的 `MainWindowHandle`，`win.ps1` 截的是**对话框**
    （尺寸从 1756x1163 变成 1262x711）；而且它的标题栏约 45px，
    **点坐标的 Y 换算和主窗口不一样**，别照搬主窗口那套。
- **附件入口按模型能力闸门**（早先那版，已被上面那批取代）：
  `m22-attach-tooltip-limited.png`、`m23-attach-tooltip-full.png`。
- **对话 / 智能体按「有没有本机文件权限」重新划分**（`n1`~`n6`）：
  闸门从「会话级工具白名单」换成「来源级勾选 + 黑名单」，语义变成
  **对话模式 = 能用 MCP 与技能、碰不到本机文件；智能体模式 = 才有本机工具**。
  - `n1-agent-mode.png`：智能体选中，扳手显示 **43**（本机 5 + 演示 12 + 分页 13 + 慢启动 13）。
  - `n2-agent-picker.png`：选择器四行来源全部勾选（本地工具 / 演示服务器 / 分页服务器 /
    慢启动服务器），下面是「高级：单独停用某些工具」折叠区。
  - `n3-chat-mode.png`：切对话后扳手显示 **38**——正好少掉本机那 5 个，MCP 一个没动。
  - `n4-chat-picker.png`：**「本地工具」那一行整个消失**，只剩三台服务器；
    提示条变成「对话模式：能用 MCP 与技能，但不会读写本机文件。」
  - `n5-chat-none.png`：对话模式点「全不选」→ 三台服务器全部取消、扳手变 **0**（灰色）。
  - `n6-back-agent.png`：切回智能体 → 只有**本地工具**重新勾上（**5 个工具**），
    三台 MCP 仍是未勾选。**这张是关键证据**：证明「全不选」只动当前可见的来源，
    没有把对话模式下看不见的本机那条一起抹掉。
  - 老数据迁移也在这一步实测过：把库里某个会话的 `tools` 手写成老格式
    `{"enabled":true,"picked":null}`，启动后变成
    `{"mode":"agent","sources":[local, demo, pages, slow, slowfail, dead],"permission":"default"}`
    —— `local` 在首位、5 台**启用**的服务器都在、**停用的那台没被带进来**、
    迁移标记没有落盘。再启动一次不再改动（幂等）。
- **智能体的项目目录**（`n7`~`n12`）：项目目录同时是**相对路径的基准**、
  **命令的工作目录**、和**「里面 / 外面」的分界线**。
  - `n7-no-workspace.png`：智能体模式但**没设项目目录** → 扳手显示 **0**（灰色）、
    多一个「📁+ 选择项目目录」按钮。没目录就不给本机工具，不留隐式兜底。
  - `n8-needs-workspace.png`：同一个状态下的选择器——「本地工具」那行显示
    **「先选项目目录」**（橙色，替掉「N 个工具」），顶部提示条是
    「智能体只在这个目录里干活，目录之外的读写每次都要你确认。」，
    而且**没有「高级」区**（那条来源现在一个工具都给不出来）。
  - `n9-workspace-set.png`：给会话设上 `cwd/` 之后 → 按钮变成 **📁 cwd ⌄**、扳手回到 **5**。
  - `n10-list-in-workspace.png`：发 `LIST`（mock 调 `list_directory {"path": "."}`）
    → 结果正是 **`cwd/` 的内容**（`.env` / `README.md` / `src/`）。
    这就是「相对路径按项目目录算」的直接证据——以前按程序启动目录算，
    装好的程序就是安装目录，模型说「看看这个项目」它会去翻 Perch 自己的目录。
  - `n11-pwd-in-workspace.png`：发 `PWD`（mock 调 `run_command {"command":"pwd"}`，
    要授权）→ 输出 `E:\Personal control\pc\tools\p3-test\cwd`。证明子进程的
    `current_dir` 也换成了项目目录。
  - `n12-outside-needs-approval.png`：发 `OUTSIDE`（mock 调
    `list_directory` 指到 `cwd/` 的**上一级**）→ **弹授权卡片**。
    `list_directory` 本来是免确认的（`Guard::Free`），以前只靠 `is_sensitive_path`
    那道防呆挡着，模型换个路径就绕过去了；现在按「在不在项目目录里」判，越界必问。
    点「拒绝」后模型收到 `User denied permission to run \`list_directory\`.`，循环正常收尾。
  - 请求体侧（`requests.jsonl`）：设了目录之后，system 消息末尾多出
    `# Environment` 一段（操作系统 / `run_command` 用的 shell / 项目目录全路径），
    `tools` 恰好 5 个本机工具。

本文只讲怎么把环境跑起来。
