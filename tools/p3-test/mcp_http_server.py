"""一个最小的 **Streamable HTTP** MCP 服务器，给 Perch 的 MCP HTTP 那条路当靶子。

和 `mcp_server.py`（stdio 那个）是一对：那边测「起子进程 + 走 stdin/stdout」，
这边测「直接连一个 URL + 带自定义请求头」。协议内容一样，只是承载方式换了。

**用法**：

    python mcp_http_server.py [端口]     默认 8770

**它按 rmcp 的 `StreamableHttpClientTransport` 的实际要求实现**（读过
`transport/common/reqwest/streamable_http_client.rs` 才写的，不是照记忆里的规范写）：

- 客户端 POST 时带的 `Accept` 是 `text/event-stream, application/json`，
  响应给 `application/json` 它就按普通 JSON-RPC 回包解析（给 `text/event-stream`
  则会当 SSE 流读，那条路这里不测）。
- **通知（没有 `id`）要回 202 Accepted 且空体**。回 200 带一个 JSON-RPC 结果
  会被当成「对通知的回复」，rmcp 那边对不上号。
- 响应的 `Content-Type` 必须以 `application/json` 开头，否则客户端直接报
  `UnexpectedContentType`——**不会**退回去看 body。
- `Mcp-Session-Id` 是可选的。这里在 `initialize` 的响应里发一个，实测客户端之后
  每次 POST 都会带回来（包括 `notifications/initialized`）——所以这条路也顺便测到了。
  ⚠️ 别拿 rmcp 源码里那句 `if uses_modern_http { None }` 反推它不带，实测就是带了。
- `DELETE` 是关会话用的。回 405 客户端就跳过（`delete_session` 里显式认这个状态码），
  这里回 405，省得还要维护会话表。
- `GET` 只在 SSE 断开重连时才用。这里回 405。

⚠️ **日志里的请求头保持线上的原始大小写**（`http` crate 的 `HeaderName` 会规范成小写，
所以实际收到的是 `x-perch-test`）。查日志时**必须不区分大小写**，用
`{k.lower(): v for k, v in headers.items()}` 再取；直接 `headers['X-Perch-Test']`
会什么都查不到，看着就像「自定义头没到」——实测踩过这一次。

**收到什么就记什么**：每个请求的完整请求头 + body 追加写进 `mcp_http_calls.jsonl`
（脚本旁边）。另外暴露一个 `headers` 工具，把**调它的那一次请求**的头原样回显——
自定义请求头有没有真的到达服务器，看这个工具的结果就知道，不用去翻日志。

工具表（够用就行，协议细节交给 stdio 那个靶子测）：

    echo      原样回显
    add       两个数相加
    headers   回显这次请求的请求头（用来验证自定义头）
    boom      永远失败
    slow      睡一会儿（测停止按钮）
"""
import json
import pathlib
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SERVER_NAME = "p3-mock-http"
PROTOCOL_VERSION = "2025-11-25"
DEFAULT_PORT = 8770
CALL_LOG = pathlib.Path(__file__).parent / "mcp_http_calls.jsonl"
SESSION_ID = "p3-http-session-1"

# 多个请求可能同时到（客户端是并发发 POST 的），日志文件要串起来写
_LOG_LOCK = threading.Lock()


def text(value):
    return {"type": "text", "text": value}


TOOLS = [
    {
        "name": "echo",
        "description": "原样返回 text。确认参数有没有原封不动地传过来。",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string", "description": "要回显的内容"}},
            "required": ["text"],
        },
    },
    {
        "name": "add",
        "description": "两个数相加。参数是数字，不是字符串。",
        "inputSchema": {
            "type": "object",
            "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
            "required": ["a", "b"],
        },
    },
    {
        "name": "headers",
        "description": "回显这次请求带的请求头。用来确认自定义头有没有到达服务器。",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "boom",
        "description": "永远失败。用来测错误结果的显示。",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "slow",
        "description": "睡一会儿再回答。用来测停止按钮。",
        "inputSchema": {
            "type": "object",
            "properties": {"seconds": {"type": "number"}},
            "required": ["seconds"],
        },
    },
]


def record(entry):
    with _LOG_LOCK:
        with CALL_LOG.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry, ensure_ascii=False) + "\n")


def handle_call(name, arguments, headers):
    if name == "echo":
        return {"content": [text(f"echo: {arguments.get('text', '')}")], "isError": False}
    if name == "add":
        total = arguments.get("a", 0) + arguments.get("b", 0)
        return {"content": [text(f"{total}")], "isError": False}
    if name == "headers":
        # 把**这一次**请求的头原样列出来。自定义头有没有到，看这里最直接。
        lines = "\n".join(f"{key}: {value}" for key, value in headers.items())
        return {"content": [text(lines)], "isError": False}
    if name == "boom":
        return {"content": [text("工具内部炸了：HTTP 这条路上的")], "isError": True}
    if name == "slow":
        seconds = min(float(arguments.get("seconds", 20)), 60)
        time.sleep(seconds)
        return {"content": [text(f"slept {seconds}s")], "isError": False}
    return {"content": [text(f"没有叫 {name} 的工具")], "isError": True}


def handle(method, params, headers):
    """返回该回给客户端的 `result`。`initialize` 那条另算（要带会话头）。"""
    if method == "initialize":
        return {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER_NAME, "version": "1.0.0"},
        }
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        params = params or {}
        return handle_call(params.get("name", ""), params.get("arguments") or {}, headers)
    if method == "ping":
        return {}
    return None


class Handler(BaseHTTPRequestHandler):
    # 用 HTTP/1.1 是为了长连接；代价是每个响应都**必须**写准 Content-Length，
    # 少写一个连接就会一直挂着等下一段。
    protocol_version = "HTTP/1.1"

    def _respond(self, status, payload=None, content_type=None, session_id=None):
        body = b"" if payload is None else json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        if content_type:
            self.send_header("Content-Type", content_type)
        if session_id:
            self.send_header("Mcp-Session-Id", session_id)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            message = json.loads(raw)
        except json.JSONDecodeError as error:
            record({"time": _now(), "error": f"body 不是 JSON：{error}", "raw": raw.decode("utf-8", "replace")})
            self._respond(400, {"error": "not json"})
            return

        method = message.get("method")
        request_id = message.get("id")
        record({
            "time": _now(),
            "method": method,
            "id": request_id,
            # 请求头全记下来：自定义头到没到、Accept 是什么、会话 id 有没有带回来
            "headers": {key: value for key, value in self.headers.items()},
            "params": message.get("params"),
        })

        if request_id is None:
            # 通知：规范要 202 + 空体。回 200 带结果会被当成「对通知的回复」而对不上号。
            self._respond(202)
            return

        result = handle(method, message.get("params"), self.headers)
        if result is None:
            self._respond(
                200,
                {"jsonrpc": "2.0", "id": request_id, "error": {"code": -32601, "message": f"没实现 {method}"}},
                content_type="application/json",
            )
            return
        # `initialize` 的响应里带上会话 id，之后客户端每次 POST 都会带回来
        session = SESSION_ID if method == "initialize" else None
        self._respond(
            200,
            {"jsonrpc": "2.0", "id": request_id, "result": result},
            content_type="application/json",
            session_id=session,
        )

    def do_DELETE(self):
        # 关会话。回 405 客户端就跳过（`delete_session` 里显式认这个码），
        # 这样不用维护会话表。
        self._respond(405)

    def do_GET(self):
        # 只在 SSE 断开重连时才用；这里不测那条路。
        self._respond(405)

    def log_message(self, fmt, *args):
        # 默认实现往 stderr 打访问日志。留着——启动脚本可以把它重定向到文件，
        # 出问题时能看到 HTTP 层发生了什么（比如客户端连错路径）。
        sys.stderr.write(f"[{SERVER_NAME}] {fmt % args}\n")
        sys.stderr.flush()


def _now():
    return time.strftime("%H:%M:%S")


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_PORT
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    sys.stderr.write(f"[{SERVER_NAME}] listening on http://127.0.0.1:{port}/mcp\n")
    sys.stderr.flush()
    server.serve_forever()
