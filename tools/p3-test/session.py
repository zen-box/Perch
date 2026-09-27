"""改隔离库里的会话状态，省得每次实测都手写一段带四层转义的 SQL。

Perch 把「这次带哪些工具、有没有项目目录、权限档」存在 `sessions.tools` 里
（`ChatSession::tools`），是 JSON 文本。实测要摆的初始状态几乎每次都不一样，
而这个字段又特别容易被 shell 的引号转义搞坏（`\\` 和 `"` 叠在一起），所以单独放一个脚本。

用法（在 `tools/p3-test/` 下跑）：

    python session.py show
    python session.py set --mode agent --permission full --workspace cwd
    python session.py set --mode chat --sources none
    python session.py set --clear                 # 只清消息
    python session.py set --mode agent --sources local,demo,pages

- `--sources` 给 `local` / `none` / 逗号分隔的服务器 **id**（`show` 会列出可用 id）。
  不给就沿用当前值。
- `--workspace` 给 `cwd`（本目录下的隔离靶子）、`none`（清掉）、或一个绝对路径。
- 会话 id 默认取第一条；多条时用 `--session <id>` 指定。
"""
import argparse
import json
import pathlib
import sqlite3
import sys

HERE = pathlib.Path(__file__).resolve().parent
DB = HERE / "appdata" / "Perch" / "perch.db"


def connect():
    if not DB.exists():
        sys.exit(f"找不到 {DB}——先把 tools/p3-test/appdata/ 摆好（见 README 第 3 步）")
    db = sqlite3.connect(DB)
    db.row_factory = sqlite3.Row
    return db


def pick_session(db, session_id):
    if session_id:
        row = db.execute("select id, title, tools from sessions where id = ?", (session_id,)).fetchone()
    else:
        row = db.execute("select id, title, tools from sessions order by rowid limit 1").fetchone()
    if row is None:
        sys.exit("库里没有会话——先启动一次 Perch 让它建一个")
    return row


def server_ids():
    """配置文件里所有 MCP 服务器的 id，`--sources` 用得上。"""
    cfg = HERE / "appdata" / "Perch" / "perch-config.json"
    if not cfg.exists():
        return []
    data = json.loads(cfg.read_text(encoding="utf-8"))
    return [s.get("id", "?") for s in data.get("mcp_servers", [])]


def parse_sources(raw):
    if raw == "none":
        return []
    out = []
    for name in raw.split(","):
        name = name.strip()
        if not name:
            continue
        out.append({"kind": "local"} if name == "local" else {"kind": "mcp", "server_id": name})
    return out


def parse_workspace(raw):
    if raw == "none":
        return None
    if raw == "cwd":
        return str(HERE / "cwd")
    return raw


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    show = sub.add_parser("show", help="打印当前会话状态")
    show.add_argument("--session")

    setter = sub.add_parser("set", help="改会话状态")
    setter.add_argument("--session")
    setter.add_argument("--mode", choices=["agent", "chat"])
    setter.add_argument("--permission", choices=["default", "full"])
    setter.add_argument("--sources", help="local / none / 逗号分隔的服务器 id")
    setter.add_argument("--workspace", help="cwd / none / 绝对路径")
    setter.add_argument("--clear", action="store_true", help="清空消息")

    args = ap.parse_args()
    db = connect()
    row = pick_session(db, args.session)

    if args.cmd == "show":
        print(f"session : {row['id']}")
        print(f"title   : {row['title']}")
        print(f"tools   : {row['tools']}")
        print(f"messages: {db.execute('select count(*) from messages where session_id = ?', (row['id'],)).fetchone()[0]}")
        ids = server_ids()
        print(f"servers : {', '.join(ids) if ids else '（配置里没有 MCP 服务器）'}")
        return

    tools = json.loads(row["tools"]) if row["tools"] else None
    if args.clear:
        db.execute("delete from messages where session_id = ?", (row["id"],))
    if args.mode or args.permission or args.sources is not None or args.workspace:
        # 从默认值起步，而不是从旧值合并：实测要的是"确定的初始状态"，
        # 半新半旧最容易把上一轮的残留当成这一轮的现象
        tools = {"mode": "chat", "sources": [], "permission": "default", "workspace": None}
        if args.mode:
            tools["mode"] = args.mode
        if args.sources is not None:
            tools["sources"] = parse_sources(args.sources)
        if args.permission:
            tools["permission"] = args.permission
        if args.workspace:
            tools["workspace"] = parse_workspace(args.workspace)
        db.execute("update sessions set tools = ? where id = ?", (json.dumps(tools, ensure_ascii=False), row["id"]))
    db.commit()
    print(f"session : {row['id']}")
    print(f"tools   : {row['tools']}")
    print(f"now     : {db.execute('select tools from sessions where id = ?', (row['id'],)).fetchone()[0]}")
    print(f"messages: {db.execute('select count(*) from messages where session_id = ?', (row['id'],)).fetchone()[0]}")


if __name__ == "__main__":
    main()
