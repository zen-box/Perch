"""把一台 Streamable HTTP 服务器写进隔离 APPDATA 的配置里，给 P3-3 阶段 G 实测用。

顺手关掉几台会拖慢启动的靶子（slowstart / slowfail 各睡 6 秒，dead 起不来），
截图里就只剩「一台 stdio + 一台 HTTP」，一眼能看清两种连接方式。
"""
import json
import pathlib

CONFIG = pathlib.Path(__file__).parent / "appdata" / "Perch" / "perch-config.json"

config = json.loads(CONFIG.read_text(encoding="utf-8"))

# 只留 demo（stdio）+ 新加的这台（HTTP）
keep_enabled = {"demo"}
for server in config["mcp_servers"]:
    server["enabled"] = server["id"] in keep_enabled

http_server = {
    "id": "httpremote",
    "name": "HTTP 演示服务器",
    "enabled": True,
    "transport": {"kind": "http", "url": "http://127.0.0.1:8770/mcp"},
    "secret_ref": "mcp/httpremote",
    "disabled_tools": [],
}

servers = [s for s in config["mcp_servers"] if s["id"] != "httpremote"]
servers.insert(0, http_server)
config["mcp_servers"] = servers

CONFIG.write_text(json.dumps(config, ensure_ascii=False, indent=2), encoding="utf-8")
print("mcp_servers:")
for server in config["mcp_servers"]:
    print(f"  {server['id']:12} enabled={server['enabled']!s:5} kind={server['transport']['kind']}")
