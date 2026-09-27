"""模拟 OpenAI Chat Completions 的流式接口，用来实测 Agent 循环。

- 每个请求体追加写进 requests.jsonl，事后核对程序到底发了什么。
- 按 OpenAI 的规则校验工具调用配对，不合规就回 400（和 OpenAI 的报错一致）。
- 回什么由最后一条用户消息里的关键词决定：
    LIST  → 调 list_directory
    RUN   → 调 run_command（echo）
    PWD   → 调 run_command（pwd，验证命令在项目目录里跑）
    OUTSIDE → 调 list_directory 列项目目录之外的路径（验证越界要授权）
    DATA  → 调 read_file 读 Perch 自己的 perch-config.json（验证完全权限下唯一的例外）
    SKILL → 调 load_skill 读一个技能（验证对话模式下也能用、且不弹授权卡片）
    SKILLFILE → 调 read_skill_file 读技能里的 docs/notes.md
    SKILLBAD  → 调 read_skill_file 读 `../secret.txt`（验证越界路径被拒）
    SLEEP → 调 run_command（睡 20 秒，测停止）
    PAR   → 一次调两个：run_command + list_directory（测并行调用）
    LOOP  → 每轮都调 list_directory（测轮数上限）
    SLOW  → 回复前先等 3 秒（测生成期间切换对话）

MCP 那一组（**从请求里的工具表按前缀挑，不写死名字**——暴露名里带服务器 id，
将来还可能带哈希，写死必然过期）。关键词取消息的第一个词，精确匹配：
    MCPCOUNT → 只报告工具总数和其中 MCP 的个数，不调工具
    MCP      → 调 echo
    MCPADD   → 调 add（参数是数字，验证 schema 有没有原样传下去）
    MCPBOOM  → 调 boom（isError=true）
    MCPBIG   → 调 big（40000 字符，测结果截断）
    MCPBLOCKS→ 调 blocks（text + image，测非文本块怎么摘要）
    MCPSTRUCT→ 调 structured（只有 structuredContent）
    MCPLONG  → 调那个超长名字的工具（测截断后的名字还能调通）
    MCPCN    → 调中文名字的工具（测兜底哈希名字还能调通）
    MCPCOLL  → 把 a.b 和 a_b 两个撞名的工具**都调一遍**（证明两个名字确实区分开了）
    MCPTWO   → 一次调两个 MCP 工具（测批量执行那条路）
    MCPBAD   → 调一个不存在的 MCP 工具名（测错误结果怎么落进会话）
  挑不到工具时回一段说明文字，而不是静默失败。
  最后一条是 tool 消息时（LOOP 除外）回一段总结文字；其余回普通文字。
"""
import json
import pathlib
import socket
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = pathlib.Path(__file__).parent / "requests.jsonl"
MCP_LOG = pathlib.Path(__file__).parent / "mcp-exposed.txt"
SKILLS_DIR = pathlib.Path(__file__).parent / "appdata" / "Perch" / "skills"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 18766


def validate(messages):
    expected = set()
    for i, msg in enumerate(messages):
        role = msg.get("role")
        if role == "tool":
            call_id = msg.get("tool_call_id")
            if call_id not in expected:
                return (f"Invalid parameter: messages with role 'tool' must be a response to a preceeding "
                        f"message with 'tool_calls'. (messages[{i}], tool_call_id={call_id!r})")
            expected.discard(call_id)
            continue
        if expected:
            return (f"An assistant message with 'tool_calls' must be followed by tool messages responding "
                    f"to each 'tool_call_id'. The following tool_call_ids did not have response messages: "
                    f"{', '.join(sorted(expected))}")
        if role == "assistant" and msg.get("tool_calls"):
            expected = {call["id"] for call in msg["tool_calls"]}
    if expected:
        return "An assistant message with 'tool_calls' must be followed by tool messages: " + ", ".join(sorted(expected))
    return None


def skill_containing(rel):
    """装了 `rel` 这个附带文件的第一个技能名。

    不按名字挑：夹具里哪个技能带 `docs/notes.md` 是会变的，按名字写死的话，
    换一个技能当靶子就会退化成"文件不存在"那条错误分支——看着像功能坏了。
    """
    root = SKILLS_DIR
    if root.is_dir():
        for path in sorted(root.iterdir()):
            if (path / rel).is_file():
                return path.name
    return first_skill_id()


def first_skill_id():
    """隔离数据目录里装着的第一个技能名。

    不写死：夹具改名之后写死的那份会静默失效，而失效的样子是"技能没装"，
    看起来跟功能坏了没区别。
    """
    names = (
        sorted(p.name for p in SKILLS_DIR.iterdir() if (p / "SKILL.md").is_file())
        if SKILLS_DIR.is_dir()
        else []
    )
    return names[0] if names else "weekly"


def text_of(msg):
    content = msg.get("content")
    if isinstance(content, list):
        return " ".join(part.get("text", "") for part in content if isinstance(part, dict))
    return content or ""


def mcp_names_of(tools):
    """请求里带 `mcp__` 前缀的工具名。"""
    return [name for name in tools if name.startswith("mcp__")]


def pick_mcp(tools, suffix):
    for name in mcp_names_of(tools):
        if name.endswith(suffix):
            return name
    return None


def pick_mcp_containing(tools, needle):
    for name in mcp_names_of(tools):
        if needle in name:
            return name
    return None


def mcp_scenario(word, tools):
    """按关键词挑要调的 MCP 工具，返回 (calls, reply)。挑不到就只回文字。"""
    total = len(mcp_names_of(tools))

    def missing(what):
        return [], f"请求里没找到{what}（一共 {total} 个 MCP 工具，全部工具 {len(tools)} 个）。"

    if word == "MCPCOUNT":
        return [], f"本次请求带了 {len(tools)} 个工具，其中 MCP 的有 {total} 个。"

    if word == "MCPBAD":
        return [("mcp__demo__definitely_not_a_tool", {})], ""

    if word == "MCPCOLL":
        # a.b 和 a_b 清洗后同名，暴露名里各带一段哈希。两个都调，验证确实区分开了
        found = [name for name in mcp_names_of(tools) if name.endswith(("_a_b", "_a_b_")) or "__a_b" in name]
        # 更稳的挑法：把以 a_b 结尾（含哈希）的都收进来
        found = [name for name in mcp_names_of(tools) if name.split("__")[-1].startswith("a_b")]
        if len(found) < 2:
            return missing("撞名的那两个工具（a.b / a_b）")
        return [(name, {}) for name in found[:2]], ""

    if word == "MCPTWO":
        echo = pick_mcp(tools, "__echo")
        add = pick_mcp(tools, "__add")
        if not echo or not add:
            return missing("echo 或 add")
        return [(echo, {"text": "第一个"}), (add, {"a": 10, "b": 20})], ""

    plans = {
        "MCP": ("__echo", {"text": "来自模型的问候"}),
        "MCPADD": ("__add", {"a": 3, "b": 4}),
        "MCPBOOM": ("__boom", {}),
        "MCPBIG": ("__big", {"chars": 40000}),
        "MCPBLOCKS": ("__blocks", {}),
        "MCPSTRUCT": ("__structured", {}),
    }
    if word in plans:
        suffix, args = plans[word]
        name = pick_mcp(tools, suffix)
        if name is None:
            return missing(f"以 {suffix} 结尾的工具")
        return [(name, args)], ""

    if word == "MCPLONG":
        name = pick_mcp_containing(tools, "a_tool_name_that_is_definitely")
        if name is None:
            return missing("名字超长的那个工具")
        return [(name, {})], ""

    if word == "MCPCN":
        # 中文名清洗后什么都不剩，暴露名是 `t<哈希>`，认不出来——用描述里的关键字反查不了，
        # 所以按"既不是已知英文名也不是哈希"来挑不现实。改成让服务端工具表顺序说话：
        # 中文工具名排在长名字后面。
        candidates = mcp_names_of(tools)
        long_index = next((i for i, n in enumerate(candidates) if "a_tool_name_that_is" in n), None)
        if long_index is None or long_index + 1 >= len(candidates):
            return missing("中文名字的那个工具")
        return [(candidates[long_index + 1], {})], ""

    return [], f"未知的 MCP 关键词 {word}。"


class Handler(BaseHTTPRequestHandler):
    counter = 0

    def log_message(self, *args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or b"{}")
        with LOG.open("a", encoding="utf-8") as f:
            f.write(json.dumps({"time": time.strftime("%H:%M:%S"), "body": body}, ensure_ascii=False) + "\n")

        messages = body.get("messages", [])
        error = validate(messages)
        if error:
            payload = json.dumps({"error": {"message": error, "type": "invalid_request_error"}}).encode()
            self.send_response(400)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return

        last_user = next((text_of(m) for m in reversed(messages) if m.get("role") == "user"), "")
        last_role = messages[-1].get("role") if messages else ""
        tools = [t["function"]["name"] for t in body.get("tools", [])]
        # 关键词取第一个词，精确匹配——MCPADD 里也含 "MCP"，用 in 判断会串台
        first_word = last_user.strip().split()[0].upper() if last_user.strip() else ""
        if mcp_names_of(tools):
            with MCP_LOG.open("a", encoding="utf-8") as f:
                f.write(json.dumps(mcp_names_of(tools), ensure_ascii=False) + "\n")
        if "SLOW" in last_user and last_role == "user":
            time.sleep(3)

        calls = []
        reply = ""
        if first_word.startswith("MCP") and last_role == "user":
            calls, reply = mcp_scenario(first_word, tools)
        elif "LOOP" in last_user:
            calls = [("list_directory", {"path": "."})]
        elif last_role == "tool":
            results = []
            for m in reversed(messages):
                if m.get("role") != "tool":
                    break
                results.append(text_of(m)[:30])
            reply = f"收到 {len(results)} 个工具结果：{list(reversed(results))!r}。结论：完成。"
        elif "SKILLBAD" in last_user:
            # 越界路径：`..` 必须被拒（`skills::safe_join` 只接受普通组件）。
            # 放在 SKILLFILE / SKILL 前面：这两个词都是 "SKILL" 的子串，顺序反了就永远走不到。
            calls = [("read_skill_file", {"name": first_skill_id(), "path": "../secret.txt"})]
        elif "SKILLFILE" in last_user:
            calls = [("read_skill_file", {"name": skill_containing("docs/notes.md"), "path": "docs/notes.md"})]
        elif "SKILL" in last_user:
            # Skills 的两个工具读的只有 Perch 自己的技能目录，所以**永远免确认**——
            # 这三条用例跑完不该出现任何授权卡片。
            calls = [("load_skill", {"name": first_skill_id()})]
        elif "PAR" in last_user:
            calls = [("run_command", {"command": "echo parallel-cmd"}), ("list_directory", {"path": "."})]
        elif "SLEEP" in last_user:
            calls = [("run_command", {"command": "Start-Sleep -Seconds 20; echo woke-up"})]
        elif "DATA" in last_user:
            # Perch **自己的数据目录**里的一个文件。用来验证「完全权限下唯一的例外」：
            # 档位开到 full 之后其它一切放行，但改自己的配置等于让模型控制程序本身。
            # 走 `read_file` 而不是 `run_command`，是因为这条底线只看 `path` 参数
            # （见 local_tools::touches_the_data_dir 的注释）。
            data_dir = pathlib.Path(__file__).resolve().parent / "appdata" / "Perch"
            calls = [("read_file", {"path": str(data_dir / "perch-config.json")})]
        elif "OUTSIDE" in last_user:
            # 项目目录**之外**的一个真实路径（仓库根，`cwd/` 的上一级）。
            # 用来验证「目录之外的读写每次都要授权」——这条才是真正的边界，
            # 以前只有 is_sensitive_path 那道防呆，模型换个路径就绕过去了。
            outside = pathlib.Path(__file__).resolve().parent.parent
            calls = [("list_directory", {"path": str(outside)})]
        elif "LIST" in last_user:
            calls = [("list_directory", {"path": "."})]
        elif "PWD" in last_user:
            # 用来验证「命令在项目目录里跑」：`list_directory "."` 证明相对路径的基准，
            # 这条证明子进程的 current_dir。两条合起来才是完整的「工作目录生效了」。
            calls = [("run_command", {"command": "pwd"})]
        elif "RUN" in last_user:
            calls = [("run_command", {"command": "echo hello-from-tool"})]
        else:
            reply = f"普通回复。本次请求带了 {len(tools)} 个工具，{len(messages)} 条消息。"

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()

        def send(obj):
            self.wfile.write(f"data: {json.dumps(obj, ensure_ascii=False)}\n\n".encode())
            self.wfile.flush()
            time.sleep(0.05)

        if calls:
            for index, (name, args) in enumerate(calls):
                Handler.counter += 1
                call_id = f"call_{Handler.counter}"
                send({"choices": [{"index": 0, "delta": {"role": "assistant", "content": None, "tool_calls": [
                    {"index": index, "id": call_id, "type": "function", "function": {"name": name, "arguments": ""}}]},
                    "finish_reason": None}]})
                arguments = json.dumps(args)
                for piece in (arguments[: len(arguments) // 2], arguments[len(arguments) // 2:]):
                    send({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": index, "function": {"arguments": piece}}]},
                                       "finish_reason": None}]})
            send({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
                  "usage": {"prompt_tokens": 100, "completion_tokens": 20}})
        else:
            for chunk in (reply[i:i + 12] for i in range(0, len(reply), 12)):
                send({"choices": [{"index": 0, "delta": {"content": chunk}, "finish_reason": None}]})
            send({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                  "usage": {"prompt_tokens": 100, "completion_tokens": 30}})
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()


def port_is_taken(port):
    """端口上已经有东西在应答。

    **Windows 上 SO_REUSEADDR 允许两个进程同时绑同一个端口**（Linux 不允许），
    所以"第二个 mock 启动成功"并不代表它接得到请求——请求会落到先绑的那个上，
    requests.jsonl 也就写在它那边。宁可启动失败，也不要对着一个空的日志排查半天。
    """
    with socket.socket() as probe:
        probe.settimeout(0.3)
        return probe.connect_ex(("127.0.0.1", port)) == 0


if __name__ == "__main__":
    if port_is_taken(PORT):
        print(
            f"端口 {PORT} 上已经有服务在跑（多半是上一次留下的 mock）。\n"
            f"先结束它再启动：请求会被它接走，requests.jsonl 写在它那边，你这边永远是空的。",
            flush=True,
        )
        raise SystemExit(1)
    print(f"mock listening on 127.0.0.1:{PORT}", flush=True)
    # 把日志路径打出来：mock 可能从别的目录启动，requests.jsonl 跟着它自己走
    print(f"requests log: {LOG}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
