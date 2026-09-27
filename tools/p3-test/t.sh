#!/usr/bin/env bash
# 实测辅助：发消息、截图、看模拟服务收到的请求。用法：source t.sh
cd "$(dirname "${BASH_SOURCE[0]}")"
W() { MSYS_NO_PATHCONV=1 powershell -NoProfile -ExecutionPolicy Bypass -File win.ps1 "$@" | tail -1; }
send() { W -Action click -X 1080 -Y 1188 > /dev/null; W -Action type -Out "$1" > /dev/null; W -Action enter > /dev/null; }
shot() { W -Action shot -Out "shots/$1.png" > /dev/null; python -c "
from PIL import Image; im=Image.open('shots/$1.png'); im.resize((im.width//2, im.height//2)).save('shots/_$1.png')"; }
nreq() { [ -f requests.jsonl ] && wc -l < requests.jsonl || echo 0; }
# 打印第 $1 条之后的请求：每条列出消息角色、工具数、tool_call_id
reqs() { python - "$1" <<'EOF'
import json, sys
start = int(sys.argv[1])
lines = open('requests.jsonl', encoding='utf-8').read().splitlines() if __import__('os').path.exists('requests.jsonl') else []
for i, line in enumerate(lines[start:], start + 1):
    body = json.loads(line)['body']
    parts = []
    for m in body['messages']:
        c = m.get('content') or ''
        c = c if isinstance(c, str) else 'parts'
        tag = m['role'][0].upper()
        if m.get('tool_calls'):
            tag += '[' + ','.join(t['function']['name'] + ':' + t['id'] for t in m['tool_calls']) + ']'
        if m['role'] == 'tool':
            tag += '(' + m.get('tool_call_id', '') + ')'
        parts.append(tag + ':' + c[:24].replace('\n', '\\n'))
    print(f"req{i} tools={len(body.get('tools', []))} | " + ' | '.join(parts))
EOF
}
