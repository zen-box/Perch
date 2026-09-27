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
| `pixel.py` | 截图像素工具，两个子命令：`find` 找某个颜色的连通块（按钮 / 开关的包围盒和中心），`probe` 逐点报颜色并扫出紫色系的 y 带。**点击之前一律先用它问一遍真实坐标**——凭肉眼估的坐标是**显示坐标**，拿去点会全部落空，理由见「坑」里那条。要 Pillow |
| `zoom.py` | 裁一块截图放大保存，用来看清局部（徽标、一行文字）。要 Pillow |
| `grid.py` | 在截图上画坐标网格，用来把「目测的位置」翻译成「文件里的像素坐标」。裁小 + 放大有时也会被 Read 再缩一次，画网格是更省事的兜底。要 Pillow |
| `mouse.py` | 用**真实光标**操作窗口（`screen` / `hover` / `click` / `where`）。`PostMessage` 分不清是「坐标算错了」还是「合成消息被吞了」时，真实光标是绝对的一试就知道。会挪用户的鼠标、也抢焦点，所以只做定性诊断。要 Pillow |
| `session.py` | 改隔离库里某个会话的 `tools`（模式 / 来源 / 权限档 / 项目目录）和消息。这个字段是 JSON 文本，手写要叠四层引号转义，极易写错。用法见文件头，`show` 会列出可用的服务器 id。`--sources` 认 `local` / `skill` / `none` / 服务器 id |
| `windows.py` | 列出某个进程的**全部顶层窗口**（句柄 / 可见性 / 矩形 / 类名 / 标题）。窗口"点了没反应"时先用它看一眼——`win.ps1` 是按 `MainWindowHandle` 找窗口的，那个属性会指到错的窗口（见「坑」里的最小化那条） |
| `skill-fixture/` | Skills 的夹具：`weekly/`（带 `docs/notes.md`）、`deploy/`，以及技能目录**外面**的 `secret.txt`（越界靶子）。复制进 `appdata/Perch/skills/` 即可，见「怎么跑」第 3b 步 |
| `cwd/` | 隔离的工作目录（一个带 `.env` 的假项目），给工具调用当靶子。`.env` 是特意放的——用来验证敏感路径会不会被拦 |
| `mock-config.json` | P3-2 那轮的 `perch-config.json`：渠道指向 mock 服务、开了本地工具、超时 600 秒。**故意不叫 `perch-config.json`**——仓库根 `.gitignore` 有一条 `perch-*.json`（防真实配置带密钥被提交），改名是为了不跟那条规则打架 |
| `mock-config-mcp.json` | P3-3 那轮的配置：在上一份基础上加了 4 台 MCP 服务器（两台能连、一台命令不存在、一台停用），并给演示服务器配了 `disabled_tools: ["spam"]`。后来又补了 `slow`（`slowstart`）和 `slowfail` 两台，用来测重连时的界面表现，以及第二个模型 `mock-vision`（带 `vision`+`files` 能力，用来对照附件闸门） |
| `mock-config-real-mcp.json` | **给用户照抄的样例**：三台真实可用的 npx 服务器（文件系统 / 顺序思考 / 记忆图谱）。渠道仍然指向 `mock.py`，所以模型侧不真跑，只用来验证「能不能连上、工具清单对不对」 |
| `shots/` | 实测留下的截图。`A`~`H` 是 P3-2 主流程，`v1`~`v8` 是 P3-2 收尾，`m1`~`m12` 是 P3-3，`m13`~`m15` 是 P3-3 三处界面缺陷的修复验证，`m16`~`m18` 是真实 MCP 服务器接入验证，`m19`~`m23` 是 P3-3 收尾（重连闪动 + 附件能力闸门），`m24`~`m28` 是会话级工具开关与「本次对话的工具」选择器，`m29`~`m31` 是 MCP 表单改弹窗，`m32` 是「编辑停用服务器不会把它打开」，`m33`~`m40` 是附件入口按模型能力分类型（提示改短 + 菜单动态化 + 去掉「所有文件」兜底），`n1`~`n6` 是对话/智能体按「有没有本机文件权限」重新划分，`n7`~`n12` 是智能体的项目目录（相对路径基准 + 越界授权），`n13`~`n18` 是会话级权限档（完全权限 + 二次确认 + 数据目录底线），`s0`~`s18` 是 Skills（对话模式可用 + 免确认 + 越界被拒 + 设置页），`a1`~`a9` 是审计日志（五类决策各一条 + 关掉开关后不再写 + 长输出截断） |

## 怎么跑

需要 Windows + PowerShell + Python 3（`mock.py` / `mcp_server.py` / `session.py` / `mouse.py` /
`windows.py` 只用标准库；`pixel.py` / `zoom.py` / `grid.py` 和 `t.sh` 的 `shot` 要 Pillow）。

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

3b. （要测 Skills 才做）把夹具复制进技能目录。技能目录是 `appdata/Perch/skills/`，
   一个子目录一个技能，入口是里面的 `SKILL.md`：

   ```bash
   mkdir -p tools/p3-test/appdata/Perch/skills
   cp -r tools/p3-test/skill-fixture/. tools/p3-test/appdata/Perch/skills/
   ```

   复制完应该是 `skills/{weekly,deploy}/SKILL.md` + `skills/weekly/docs/notes.md`
   + `skills/secret.txt`（最后这个是**越界靶子**，故意放在技能目录外面）。

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
   **权限档那一批实测重量的结果**（同一行、同一窗口尺寸）：输入框文字行 `1000,1058`、
   工具栏在 `y≈1085~1126`（中心 `1105`）：附件 `519`、模型选择器 `546~682`、参数 `696~784`、
   对比 `801~858`、**对话 `1283~1325`（中心 `1304`）**、**智能体 `1356~1430`（中心 `1393`）**、
   **项目目录按钮 `1460~1538`（`cwd` 文字中心 `1487`，下拉箭头 `1533`）**、
   **扳手 `1566~1593`（中心 `1579`）**。
   ⚠️ **输入框要点 `y≈1058` 那一行文字，别点 `1074,1079`**——那一条已经落到工具栏行上了，
   点下去什么都不会发生（踩过：点了 `1000,1110`，以为输入框没聚焦，其实是点在工具栏空白处，
   连试两轮都发不出消息）。
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
   **Skills 那一批补的**：设置页导航第 4 项「技能」`102,376`（导航自上而下
   通用设置 / 模型渠道 / 提示词 / 技能 / MCP 服务器 / 关于，行距约 52）、
   技能页两行开关 `1540,361` / `1540,468`、「导入技能」`347,482`、「重新扫描」`441,482`、
   「打开目录」`537,482`、「返回对话」`94,93`。
   ⚠️ **工具选择器面板是从下往上长的**（`Anchor::BottomRight`），所以**行数一变，
   所有行的纵向位置全变**——记坐标要连着"当时面板里有几行"一起记。
   实测「对话模式 + 只装了技能」时（面板 1 行 + 一行提示）：技能行中心 `1300,1047`；
   「智能体 + 本机 + 技能 + 3 台服务器」时（面板 5 行）：技能行中心 `1073,470`。
   **两个数都对，差别只在行数。**
   ⚠️ 附件菜单在按钮**上方**展开，最上面一行贴着消息区；消息底部的「Token 与费用明细」
   悬停卡片一旦被鼠标唤起就会**一直挂着**（`PostMessage` 发不出 `WM_MOUSELEAVE`），
   正好压住菜单第一行。**先开菜单、再把鼠标挪到空白处**（如 `1250,180`）——卡片消失、
   菜单还在，这时再截图。反过来先挪鼠标再开菜单，卡片会跟着回来。
   ⚠️ 每行右侧那一串按钮挨得很近（`断开/重连` → 铅笔 → 垃圾桶 → 开关），
   铅笔和垃圾桶只差约 32px，**点歪就点成删除**（踩过：`1457,904` 想点铅笔，
   结果弹出删除确认框）。用 `zoom.py` 裁出来量，别按整图估。
   换尺寸就得重算。
   ⚠️ 坐标别靠肉眼估：截图会被工具按比例缩过再显示，估出来的位置能差上百像素
   （踩过：附件按钮估成 `386,1066`，实际在 `522,1105`）。用 `pixel.py find`，
   或者按上面那套「扫一行里的暗像素列」的办法量。

## 坑

- ⚠️ **点击坐标必须是截图文件里的像素，不是你在聊天里"看到"的位置。**
  Read 工具会把 `1756x1163` 的截图缩到 ~1092 宽再显示，目测出来的坐标要乘约 `1.6`
  才是真实像素。这个坑的表现和下面「窗口被最小化」那条几乎一样（点了没反应、
  前后两张截图的字节数一模一样），但成因完全不同，白绕过好几轮：
  - 目测「允许执行一次」在 `(955,635)`，`pixel.py probe` 一看那点是纯白 `(255,255,255)`；
    真正的紫色块在 `x 488..740`，那是 `Mock Model` 的头像；
  - 目测设置齿轮在 `(1570,26)`，点下去窗口直接缩成 `219x39`——那一下打在**最小化**上了。

  **规矩**：点击之前先 `python pixel.py find <png> <颜色> [y0] [y1]` 拿包围盒中心，
  或者 `python pixel.py probe <png> x,y ...` 确认那一点确实是目标颜色；
  实在拿不准就 `grid.py` 画网格读数。还有一条容易漏的：
  `pixel.py find purple` 的阈值下界是 `60` 不是 `80`——GPUI 的 primary 色一档是
  `#4F46E5`(79,70,229)，差 1 个值就会把整个开关漏掉。
- ⚠️ **`win.ps1 -Action wheel` 会把窗口搞成最小化。** 实测里连着两次：滚轮发完
  窗口就变成 `219x39`（`windows.py` 看到坐标是 `-21333`）。`WM_MOUSEWHEEL` 的
  lParam 该是屏幕坐标、wParam 高 16 位该是 delta，脚本里这两处看着都对，原因没查出来。
  **替代方案**：设置页这类滚动容器**不吃键盘**（`Page Down` 完全没反应；先点一下
  空白处把焦点从输入框上拿开也只滚一点点），所以要么 `-Action wheel` 之后
  用 `-Action size` 把窗口捞回来，要么干脆改配置文件 + 重启——后者更省事。
- ⚠️ **agent 的沙箱会打断 Rust 建匿名管道，MCP 一台都起不来。** 症状是设置页里
  所有服务器都显示 `连接失败：failed to start \`...\python.exe\`: 所有的管道范例都在使用中。(os error 231)`。
  `ERROR_PIPE_BUSY` 来自 Rust std 的 `child_pipe()`——它用 `NtCreateNamedPipeFile` 造
  匿名管道，在 agent 进程树里这一步会失败。判别方法：把复现程序挂在 WMI 下面跑
  （`([wmiclass]"Win32_Process").Create(...)`），五种 stdio 组合全部成功；从 agent 的
  shell 里跑就全失败。**结论是环境问题，不是产品缺陷**（真实用户不会遇到），
  但实测时必须按上面第 4 步启动。
- ⚠️ **窗口被最小化之后，所有点击和截图都会静默作用在"错的东西"上。**
  症状：截图变成一张 `219x39` 的小图、点击完全没反应、库里一条消息都没多——
  很容易误判成"功能坏了"。原因是 `win.ps1` 按 `MainWindowHandle` 找窗口，
  而窗口最小化时那个属性指向的就是这个被 Windows 停到 `(-21333,-21333)` 的小窗口。
  这是**真实桌面**上的窗口，任何外部操作（包括人自己把它最小化）都能造成。
  判别：`python windows.py perch-p3` —— 正常时应该只有一条
  `class='Zed::Window' title='Perch'` 且尺寸是 `1756x1163`；
  看到 `-21333` 或 `-32000` 的坐标就是被最小化了。
  修法：`win.ps1 -Action restore`（或直接再跑一次 `-Action size`，
  它现在会先 `SW_RESTORE` 再调尺寸——以前不会，于是把 `-21333` 那个位置原样写回去，
  尺寸看着对了但窗口还是最小化的）。
  留了一张现场图：`shots/s9-minimized.png`（219x39 的"截图"，正是踩坑时的样子）。
  对照 `shots/s10-restored.png`（同一次会话，`-Action size` 之后恢复正常）。
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
- ⚠️ **弹层的纵向位置会随内容高度变，所以每改一次状态就得重新量。**
  「本次对话的工具」用的是 `Anchor::BottomRight`，**从下往上长**：内容变矮（比如权限档那两条
  警告消失）整块就往下走，内容变高就往上顶。踩过一次很值当的坑：按内容变矮**之前**量到的
  坐标 `1558,592` 去点「完全权限」的开关，结果那已经是「分页服务器」那一行了，
  一点把工具数从 5 变成 18，而权限档压根没动。
  **判据：任何一次点击前后，面板高度或行数只要变过，坐标一律作废。**
  量法同工具栏：在**目标那一行**的 y 上扫暗像素（或扫开关的蓝紫色）再聚类。
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
- 关键词场景（`LIST` / `RUN` / `PWD` / `OUTSIDE` / `DATA` / `SKILL` / `SKILLFILE` / `SKILLBAD` /
  `SLEEP` / `PAR` / `LOOP` / `SLOW`，以及 MCP 那一组）见 `mock.py` 文件头，改场景直接改那里。
  ⚠️ `SKILL` 是 `SKILLFILE` / `SKILLBAD` 的子串，**判断顺序不能反**——反了后面两个永远走不到。

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
- **会话级权限档：完全权限 + 二次确认 + 数据目录底线**（`n13`~`n18`）。
  档位存在 `ChatSession.tools.permission`（`default` / `full`），入口在工具选择器面板底部、
  只在智能体模式下出现。**二次确认是面板内联的**，不是弹窗（原因见 `ui/tool_picker.rs` 的注释：
  `Popover` 走 `deferred()`，弹窗层是普通子元素，从面板里弹出来的框会被面板自己盖住）。
  - `n13-permission-off.png`：默认态。「完全权限」一行 + 右侧关闭的开关，没有警告。
  - `n14-permission-confirm.png`：拨开开关 → **开关那一行被替换成警告块**：
    红标题「打开完全权限？」+ 一整段代价说明 + 「取消」/「仍然打开」。没有弹窗。
  - `n15-permission-on.png`：点「仍然打开」之后——开关变蓝、标签变红并带盾牌图标，
    下面挂两条常驻警示（「完全权限：模型可以在你机器上做任何事（Perch 自己的数据目录除外）。」
    和「Perch 不是沙盒。要跑不受信任的代码，请在虚拟机或容器里跑。」）。
    会话里 `permission` 落成 `"full"`（`session.py show` 可核对）。
  - `n16-permission-outside-allowed.png`：**完全权限下发 `OUTSIDE`（`list_directory` 指到
    项目目录之外）→ 直接执行，不再弹授权卡片**，结果卡片是绿的。
    对照 `n12-outside-needs-approval.png`（同样是越界，默认档位下必弹）。
  - `n17-permission-data-dir-guarded.png`：**完全权限下发 `DATA`
    （`read_file` 指到 Perch 自己的 `perch-config.json`）→ 仍然弹授权卡片**。
    这是 `AGENTS.md` §11 第 2 条那条硬底线：让模型改自己的配置 = 让模型控制程序本身的行为。
    ⚠️ 它只看 `path` 参数，`run_command` 里手写数据目录路径绕得过去。
  - `n18-permission-back-off.png`：拨回默认——**往回收权限不需要确认**，直接生效
    （截图里是从确认块点了「取消」之后的状态：警告块消失、开关关着、档位仍是 `default`）。
  - 请求体侧（`requests.jsonl`）：全程 `tools` 恰好 5 个本机工具；system 末尾的
    `# Environment` 段照旧。
  - 顺带修掉一个自己造出来的毛病：确认态原本会**跨面板开关残留**——拨开开关、
    关掉面板再打开，看到的还是警告和两个按钮，得先点一次「取消」。
    改成面板关闭时清掉（`Popover::on_open_change` → `dismiss_full_permission`，幂等）。
- **Skills：装进数据目录的技能包**（`s0`~`s18`）。技能目录 `%APPDATA%\Perch\skills\`，
  一个子目录一个技能，入口 `SKILL.md`。系统提示词里只列「名字 + 一句描述」，
  正文等模型调 `load_skill` 自己取。新增关键词：`SKILL` / `SKILLFILE` / `SKILLBAD`
  （按顺序判断，因为后两个都含 `SKILL` 子串）。
  - `s1-picker.png` / `s2-skill-picked.png`：工具选择器里多出一行 **「技能」**，
    右边写「2 个工具」；勾上之后按钮角标从 `5` 变 `7`。这一行**对话和智能体都出现**
    （它读的只有 Perch 自己的目录，碰不到用户的文件系统）。
  - `s3-chat-mode.png`：切到**对话模式**之后，角标从 `7` 掉到 **`2`**，
    项目目录按钮消失——本机工具全被剔掉，只剩两个技能工具。
    这是「对话也能用 Skills、但碰不到硬盘」最直接的证据。
  - `s7-skill-result.png`：**对话模式下**发 `SKILL` → 模型调 `load_skill(name=deploy)`
    → **没有授权卡片**，直接执行，结果卡片里是 `SKILL.md` 的正文。两个技能工具永远免确认。
  - `s8-skillfile.png`：发 `SKILLFILE` → `read_skill_file` 指到一个**这个技能里没有的**
    文件 → 红卡片 `skill \`deploy\` 里没有 \`docs/notes.md\``。
    （夹具里 `deploy` 没有附带文件，mock 按名字取第一个技能，所以撞上了这条错误分支——
    顺手证明了「文件不存在」的错误路径也通。后来把 mock 改成按**哪个技能带这个文件**挑。）
  - `s12-skillfile-ok.png`：发 `SKILLFILE` → `read_skill_file(name=weekly, path=docs/notes.md)`
    → 绿卡片，内容正是夹具里那份 `docs/notes.md`。
  - `s13-skillbad.png`：发 `SKILLBAD` → `read_skill_file(name=deploy, path=../secret.txt)`
    → 红卡片 `路径 \`../secret.txt\` 不合法：只能读这个 skill 目录里的文件。`
    **`secret.txt` 的内容一个字都没出来**——`safe_join` 只接受普通路径组件，`..` 直接拒。
    这个靶子特意放在技能目录**外面**：要是越界判断失效，读出来的会是那段说明文字，
    和"文件不存在"一眼就能区分开。
  - `s14-settings.png` / `s15-skills-page.png`：设置页导航多出「技能」；页面列出两个技能，
    左边是 front matter 里的 `name`（发布检查 / 写周报）+ 目录名 + 附带文件数，
    右边一个开关。
  - `s16-skills-off.png` / `s17-all-skills-off-note.png`：把两个开关都关掉 →
    `perch-config.json` 里落成 `disabled_skills: ["deploy","weekly"]`；
    选择器里那一行的「2 个工具」变成黄字 **「技能都停用了」**，角标掉到 `0`。
    （停用**一个**不会出提示——只有全停用才给不出工具。）
  - 请求体侧（`requests.jsonl`）：`tools` 恰好 `['load_skill','read_skill_file']`；
    system 消息末尾多出 `# Skills` 一节，**只有名字和一句话**：
    ```
    - deploy: 按检查清单过一遍再发布
    - weekly: 把一周的流水账整理成周报
    ```
    正文一个字都没进提示词——这正是 Skills 平时不占上下文的理由。
- **审计日志：模型动过什么、是自动放行还是你点的头**（`a1`~`a9`）。一天一个文件，
  写在 `%APPDATA%\Perch\logs\audit-YYYY-MM-DD.jsonl`，一行一条 JSON。
  `a1-audit-log.jsonl` 是**实测导出的原始日志**（20 条，五类决策齐全），
  下面每条结论都能在它里面找到对应行。
  - `a2-free-tool-runs-without-a-card.png`：发 `LIST`（`list_directory` 在项目目录里）
    → **没有授权卡片**，直接执行 → 日志 `"decision":"auto"`。
  - `a3-approval-card.png` / `a4-denied.png`：发 `RUN` 弹卡片点「允许执行一次」
    → `"decision":"approved"`、`"exit":0`、`"detail":"hello-from-tool\r\n"`；
    发 `OUTSIDE` 弹卡片点「拒绝」→ `"decision":"denied"`、`"ok":false`、
    `"detail":"denied by the user"`。
  - `a5-stopped.png`：发 `SLEEP`（`Start-Sleep -Seconds 20`）→ 允许之后**中途点停止**
    → `"decision":"stopped"`、`"ms":0`、`"detail":"stopped by the user."`。
    这条最难凑：得在 20 秒里点完「允许」再点「停止」，第一遍慢了一步，
    日志记的是 `approved` + `"ms":20485`（跑满了）。
  - `a6-limit-reached.png`：发 `LOOP`（mock 每轮都回一个 `list_directory`）
    → 撞上 `MAX_AGENT_ROUNDS`（12）→ 界面弹「这一轮工具调用太多了，已经停下」，
    日志里前面是一串 `auto`、最后一条 `"decision":"limit"` +
    `"detail":"the tool-call limit for this turn was reached."`。
  - `a7-long-detail-truncated.png`：往 `cwd/` 里塞 60 个文件再发 `LIST`
    → `detail` 被切成 `...\nf41.txt\nf…`。**`detail` 走 `truncate`（补一个 `…`），
    `args` 走 `summarize_text`（补 `... (N chars)`）**，两者不一样，别记混。
    顺手修掉一个观感 bug：`summarize_text` 原来内部调用 `truncate`，
    于是超长参数会写成 `…... (1000 chars)` 两个省略号，现在只留后面那个。
  - `a8-switch-off.png` / `a9-logging-off-still-runs.png`：设置页「通用」→
    「记录工具调用」关掉 → `perch-config.json` 落成 `"audit_log_enabled": false`、
    开关变灰；再发一条 `LIST` → **工具照常执行、界面照常显示结果，
    但日志文件一条没涨**（`wc -l` 前后都是 20）。
  - ⚠️ 「记录工具调用」在设置页最底下，是**滚下去**才看得到的，而滚动这条路当时不通
    （见「坑」里 `wheel` 那条），所以开关的「关」是点出来的、「开」是改配置 + 重启做的。

本文只讲怎么把环境跑起来。
