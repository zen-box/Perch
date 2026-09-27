"""模拟 OpenAI Chat Completions 的流式接口，用来实测 Agent 循环。

- 每个请求体追加写进 requests.jsonl，事后核对程序到底发了什么。
- 按 OpenAI 的规则校验工具调用配对，不合规就回 400（和 OpenAI 的报错一致）。
- 回什么由最后一条用户消息里的关键词决定：
    LIST  → 调 list_directory
    RUN   → 调 run_command（echo）
    SLEEP → 调 run_command（睡 20 秒，测停止）
    PAR   → 一次调两个：run_command + list_directory（测并行调用）
    LOOP  → 每轮都调 list_directory（测轮数上限）
    SLOW  → 回复前先等 3 秒（测生成期间切换对话）
  最后一条是 tool 消息时（LOOP 除外）回一段总结文字；其余回普通文字。
"""
import json
import pathlib
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = pathlib.Path(__file__).parent / "requests.jsonl"
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


def text_of(msg):
    content = msg.get("content")
    if isinstance(content, list):
        return " ".join(part.get("text", "") for part in content if isinstance(part, dict))
    return content or ""


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
        if "SLOW" in last_user and last_role == "user":
            time.sleep(3)

        calls = []
        reply = ""
        if "LOOP" in last_user:
            calls = [("list_directory", {"path": "."})]
        elif last_role == "tool":
            results = []
            for m in reversed(messages):
                if m.get("role") != "tool":
                    break
                results.append(text_of(m)[:30])
            reply = f"收到 {len(results)} 个工具结果：{list(reversed(results))!r}。结论：完成。"
        elif "PAR" in last_user:
            calls = [("run_command", {"command": "echo parallel-cmd"}), ("list_directory", {"path": "."})]
        elif "SLEEP" in last_user:
            calls = [("run_command", {"command": "Start-Sleep -Seconds 20; echo woke-up"})]
        elif "LIST" in last_user:
            calls = [("list_directory", {"path": "."})]
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


if __name__ == "__main__":
    print(f"mock listening on 127.0.0.1:{PORT}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
