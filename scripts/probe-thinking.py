"""Probes how an OpenAI-compatible server reacts to thinking parameters for one model (task 0361).

Use it when a model is added, then record what it shows in
crates/fm-application/src/model_thinking.json (`levels`, `switch`, `tested`).

Usage:
  python3 -I scripts/probe-thinking.py BASE_URL MODEL [API_KEY_ENV]
  PROMPT="Is 9991 prime? If not, give its factors." VARIANTS="plain,kwargs off,effort high" \
    python3 -I scripts/probe-thinking.py http://127.0.0.1:8000 my-model

Prints, per variant: HTTP status, seconds, reasoning characters, answer, and the error (trimmed).
A server that ignores unknown values answers 200 for `effort bogus`; one that checks them answers
400. The API key, when given, is read from the named environment variable and never printed.
"""

import json
import os
import sys
import time
import urllib.error
import urllib.request

base, model = sys.argv[1], sys.argv[2]
key = os.environ.get(sys.argv[3], "") if len(sys.argv) > 3 else ""

VARIANTS = {
    "plain": {},
    "kwargs off": {"chat_template_kwargs": {"enable_thinking": False}},
    "kwargs on": {"chat_template_kwargs": {"enable_thinking": True}},
    "thinking off": {"chat_template_kwargs": {"thinking": False}},
    "effort none": {"reasoning_effort": "none"},
    "effort minimal": {"reasoning_effort": "minimal"},
    "effort low": {"reasoning_effort": "low"},
    "effort medium": {"reasoning_effort": "medium"},
    "effort high": {"reasoning_effort": "high"},
    "effort xhigh": {"reasoning_effort": "xhigh"},
    "effort bogus": {"reasoning_effort": "bogus"},
    "on + effort low": {"chat_template_kwargs": {"enable_thinking": True}, "reasoning_effort": "low"},
}
only = os.environ.get("VARIANTS")
if only:
    VARIANTS = {name: VARIANTS[name] for name in only.split(",")}

for name, extra in VARIANTS.items():
    body = {
        "model": model,
        "messages": [{"role": "user", "content": os.environ.get("PROMPT", "What is 17*23? Reply with the number only.")}],
        "max_tokens": 1500,
        "stream": False,
        **extra,
    }
    request = urllib.request.Request(
        f"{base}/v1/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", **({"Authorization": f"Bearer {key}"} if key else {})},
    )
    started = time.time()
    try:
        with urllib.request.urlopen(request, timeout=600) as response:
            status, text = response.status, response.read().decode()
    except urllib.error.HTTPError as error:
        status, text = error.code, error.read().decode(errors="replace")
    except Exception as error:  # noqa: BLE001 - a probe reports anything
        status, text = 0, str(error)
    seconds = time.time() - started
    reasoning, answer, problem = 0, "", ""
    try:
        data = json.loads(text)
        message = data["choices"][0]["message"]
        thought = message.get("reasoning_content") or message.get("reasoning") or ""
        content = message.get("content") or ""
        if "<think>" in content:
            inner, _, content = content.partition("</think>")
            thought += inner
        reasoning, answer = len(thought), content.strip()[:30]
    except Exception:  # noqa: BLE001
        problem = " ".join(text.split())[:160]
    print(f"{name:16} {status:3} {seconds:6.1f}s reasoning={reasoning:5} answer={answer!r} {problem}")
