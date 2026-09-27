"""一个最小的 stdio MCP 服务器，给 Perch 的 MCP 那条路当靶子。

**分帧**：stdin / stdout 上走**按行切**的 JSON-RPC 2.0（rmcp 的
`JsonRpcMessageCodec` 就是找 `\\n` 切的，顺带容忍 `\\r`）。所以往 stdout 写任何
非协议内容都会直接把会话搞坏——要打日志一律走 stderr，那一边 Perch 会收进日志面板。

**用法**（配置里 `args` 的第一个参数就是模式）：

    (无) / serve   正常服务，暴露下面整张工具表
    small          只暴露 echo / add 两个（用来测两台服务器并存）
    paged          工具分页返回，每页 2 个（用来测 list_all_tools 的翻页）
    slowstart      和 serve 一样，但**握手前先睡 6 秒**（用来测「重连时界面闪不闪」：
                   有这几秒的窗口才截得到 Connecting 那一帧，否则连接一闪而过）
    slowfail       睡 6 秒再往 stderr 报一句错退出（测「点重连时失败原因那一行
                   消失又出现，整行高度跟着跳」）
    crash          往 stderr 打一行就退出（测「进程起不来」）
    garbage        往 stdout 吐非 JSON（测「握手时协议被脏数据打断」）
    mute           起得来但从不回应（测握手超时，要等满 60 秒）

工具表是故意挑的，每条对应一个容易写错的路径：

    echo        最普通的一条；每次调用往 stderr 写一行（测日志面板）
    add         参数是数字不是字符串（测 schema 原样透传给模型）
    slow        睡 20 秒（测停止按钮能不能把在跑的调用掐掉）
    boom        返回 isError=true（测错误结果怎么显示）
    big         返回超过 30_000 字符（测结果截断）
    blocks      返回 text + image 两块（测非文本块的摘要写法）
    structured  只给 structuredContent，不给 content（测兜底）
    spam        往 stderr 写 250 行（测日志环形缓冲的上限）
    a.b / a_b   清洗之后同名（测撞名怎么区分）
    <超长名>     超过 64 字符（测截断必须带哈希）
    中文工具名    清洗后什么都不剩（测兜底成哈希）

收到的每个请求追加写进 `mcp_calls.jsonl`（写在脚本旁边），事后核对 Perch 到底
发了什么——尤其是**发给服务器的工具名是原始的、不带 `mcp__` 前缀**这一点。
"""
import json
import pathlib
import sys
import time

SERVER_NAME = "p3-mock"
PROTOCOL_VERSION = "2025-11-25"
CALL_LOG = pathlib.Path(__file__).parent / "mcp_calls.jsonl"

MODE = sys.argv[1] if len(sys.argv) > 1 else "serve"


def log(message):
    """往 stderr 写一行。stdout 是协议通道，绝不能碰。"""
    sys.stderr.write(f"[{SERVER_NAME}] {message}\n")
    sys.stderr.flush()


def text(value):
    return {"type": "text", "text": value}


def tool(name, description, properties=None, required=None, title=None):
    entry = {"name": name, "inputSchema": {"type": "object", "properties": properties or {}}}
    if description is not None:
        entry["description"] = description
    if title is not None:
        entry["title"] = title
    if required:
        entry["inputSchema"]["required"] = required
    return entry


# 超过 64 字符：暴露给模型的名字必须被截断，而截断之后还得能区分开
LONG_NAME = (
    "a_tool_name_that_is_definitely_longer_than_the_sixty_four_character_limit"
    "_and_therefore_has_to_be_shortened_by_the_client"
)

TOOLS = [
    tool("echo", "原样返回 text。用来确认参数有没有原封不动地传过来。",
         {"text": {"type": "string", "description": "要回显的内容"}}, ["text"]),
    tool("add", "两个数相加。参数是数字，不是字符串。",
         {"a": {"type": "number"}, "b": {"type": "number"}}, ["a", "b"]),
    tool("slow", "睡一会儿再回答。用来测停止按钮。",
         {"seconds": {"type": "number", "description": "睡多少秒，最多 60"}}, ["seconds"]),
    tool("boom", "永远失败。用来测错误结果的显示。", {}),
    tool("big", "返回一大段文本。用来测结果截断。",
         {"chars": {"type": "number"}}, ["chars"]),
    tool("blocks", "返回文本加一张图。用来测非文本块怎么摘要。", {}),
    tool("structured", "只给 structuredContent，content 是空的。", {}),
    tool("spam", "往 stderr 写 250 行。用来测日志环形缓冲。", {}),
    tool("a.b", "名字里有点号，清洗后会变成 a_b。", {}),
    tool("a_b", "名字里是下划线，清洗后还是 a_b——和上面那个撞名。", {}),
    tool(LONG_NAME, "名字超长，暴露给模型时必须截断。", {}),
    tool("中文工具名", "名字全是非 ASCII，清洗后什么都不剩。", {}),
    tool("titled", None, {}, title="只有 title 没有 description"),
]

SMALL_TOOLS = [TOOLS[0], TOOLS[1]]


def tools_for_mode():
    if MODE == "small":
        return SMALL_TOOLS
    return TOOLS


def reply(request_id, result):
    payload = {"jsonrpc": "2.0", "id": request_id, "result": result}
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def reply_error(request_id, code, message):
    payload = {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def record(entry):
    with CALL_LOG.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry, ensure_ascii=False) + "\n")


def handle_tools_list(request_id, params):
    tools = tools_for_mode()
    if MODE != "paged":
        reply(request_id, {"tools": tools})
        return
    # 每页 2 个。客户端要是不翻页，就只会拿到前 2 个工具——静默少工具很难查，
    # 所以这条路径值得单独测。
    cursor = (params or {}).get("cursor")
    start = int(cursor) if cursor else 0
    page = tools[start:start + 2]
    result = {"tools": page}
    if start + 2 < len(tools):
        result["nextCursor"] = str(start + 2)
    reply(request_id, result)


def handle_tools_call(request_id, params):
    name = (params or {}).get("name", "")
    arguments = (params or {}).get("arguments") or {}
    record({"time": time.strftime("%H:%M:%S"), "tool": name, "arguments": arguments})
    log(f"call {name} arguments={json.dumps(arguments, ensure_ascii=False)}")

    if name == "echo":
        reply(request_id, {"content": [text(f"echo: {arguments.get('text', '')}")], "isError": False})
    elif name == "add":
        total = arguments.get("a", 0) + arguments.get("b", 0)
        reply(request_id, {"content": [text(f"{total}")], "isError": False})
    elif name == "slow":
        seconds = min(float(arguments.get("seconds", 20)), 60)
        log(f"sleeping {seconds}s")
        time.sleep(seconds)
        reply(request_id, {"content": [text(f"slept {seconds}s")], "isError": False})
    elif name == "boom":
        reply(request_id, {"content": [text("工具内部炸了：拿不到那个文件")], "isError": True})
    elif name == "big":
        chars = int(arguments.get("chars", 40000))
        reply(request_id, {"content": [text("x" * chars)], "isError": False})
    elif name == "blocks":
        reply(request_id, {
            "content": [
                text("这是文本块"),
                {"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"},
            ],
            "isError": False,
        })
    elif name == "structured":
        reply(request_id, {"content": [], "structuredContent": {"answer": 42, "unit": "次"}, "isError": False})
    elif name == "spam":
        for index in range(250):
            log(f"spam line {index:03d}")
        reply(request_id, {"content": [text("写了 250 行日志")], "isError": False})
    elif name in ("a.b", "a_b"):
        reply(request_id, {"content": [text(f"我是 {name}")], "isError": False})
    elif name == LONG_NAME:
        reply(request_id, {"content": [text("长名字的工具被调到了")], "isError": False})
    elif name == "中文工具名":
        reply(request_id, {"content": [text("中文名字的工具被调到了")], "isError": False})
    elif name == "titled":
        reply(request_id, {"content": [text("title 兜底描述的那个工具")], "isError": False})
    else:
        reply(request_id, {"content": [text(f"没有叫 {name} 的工具")], "isError": True})


def serve():
    log(f"started, mode={MODE}, {len(tools_for_mode())} tools")
    # 顺带验证一下 Perch 的 stderr 读取器会不会把 ANSI 转义剥掉
    sys.stderr.write("\x1b[31m[%s] 这行带 ANSI 颜色，界面上不该看到转义符\x1b[0m\n" % SERVER_NAME)
    sys.stderr.flush()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            log(f"stdin 上收到不是 JSON 的内容：{error}")
            continue

        method = message.get("method")
        request_id = message.get("id")
        params = message.get("params")
        if request_id is None:
            # 通知，不用回
            log(f"notification {method}")
            continue
        if method == "initialize":
            requested = (params or {}).get("protocolVersion")
            log(f"initialize (客户端要 {requested})")
            reply(request_id, {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER_NAME, "version": "1.0.0"},
            })
        elif method == "tools/list":
            handle_tools_list(request_id, params)
        elif method == "tools/call":
            handle_tools_call(request_id, params)
        elif method == "ping":
            reply(request_id, {})
        else:
            reply_error(request_id, -32601, f"没实现 {method}")


if __name__ == "__main__":
    if MODE == "slowstart":
        # 起得来，但握手前先磨蹭几秒。用来测「重连时界面会不会闪」：
        # 工具清单如果被清掉，这几秒里详情区会塌成一行「暂无工具」再撑回来。
        time.sleep(6)
        serve()
        raise SystemExit(0)
    if MODE == "slowfail":
        # 磨蹭几秒再失败。用来复现「点重连时页面闪动」：这几秒里状态是 Connecting，
        # 行内那条红色失败原因会消失（整行矮一截），失败后又长回来。
        time.sleep(6)
        sys.stderr.write("故意失败：这个服务器就是起不来\n")
        sys.stderr.flush()
        raise SystemExit(4)
    if MODE == "crash":
        log("这个服务器故意起不来")
        raise SystemExit(3)
    if MODE == "garbage":
        # 协议被脏数据打断：握手时客户端会读到一行不是 JSON 的东西
        sys.stdout.write("this is not json at all\n")
        sys.stdout.flush()
        for _ in sys.stdin:
            pass
        raise SystemExit(0)
    if MODE == "mute":
        log("起来了，但从现在开始不回应任何请求")
        for _ in sys.stdin:
            pass
        raise SystemExit(0)
    serve()
