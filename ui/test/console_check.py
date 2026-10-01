#!/usr/bin/env python3
"""Load every harness scenario headless and count console errors.
Reuses shots_cdp.py's scenario list + chrome; listens to Runtime.consoleAPICalled
and Runtime.exceptionThrown for 8s per page. Prints per-scenario error counts.
"""
import json
import os
import subprocess
import sys
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from shots_cdp import (CHROME, BASE, PORT, SCENARIOS, Ws, find_target)  # noqa: E402


def main():
    only = set(sys.argv[1:])
    bad = 0
    proc = subprocess.Popen(
        [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
         f"--remote-debugging-port={PORT}", "--window-size=1280,900",
         "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                find_target(); break
            except Exception:
                time.sleep(0.3)
        else:
            raise RuntimeError("chrome did not come up")
        try:
            tgt = find_target()
            ws = Ws(tgt["webSocketDebuggerUrl"])
            ws.cmd("Runtime.enable")
            ws.cmd("Page.enable")
        except Exception:
            raise RuntimeError("cdp did not come up")
        for suffix, out, (w, h) in SCENARIOS:
            if only and out not in only:
                continue
            ws.cmd("Emulation.setDeviceMetricsOverride", width=w, height=h,
                   deviceScaleFactor=1, mobile=False)
            errors = []
            ws.eval_errors = errors  # not used by Ws; collected below
            try:
                ws.cmd("Page.navigate", url=f"{BASE}/test/ui_harness.html?{suffix}")
            except Exception as e:
                # reconnect (ws dropped) and count the scenario as failed-safe
                print(f"{out}: NAV-FAIL {e}")
                bad += 1
                try:
                    tgt = find_target()
                    ws = Ws(tgt["webSocketDebuggerUrl"])
                    ws.cmd("Runtime.enable")
                    ws.cmd("Page.enable")
                except Exception:
                    pass
                continue
            deadline = time.time() + 8.0
            while time.time() < deadline:
                try:
                    msg = json.loads(ws_recv_msg(ws, timeout=max(0.05, deadline - time.time())))
                except (TimeoutError, EOFError, OSError):
                    break
                if msg.get("method") == "Runtime.exceptionThrown":
                    d = msg["params"]["exceptionDetails"]
                    text = d.get("exception", {}).get("description") or d.get("text")
                    errors.append("EXC " + str(text)[:200])
                elif msg.get("method") == "Runtime.consoleAPICalled":
                    p = msg["params"]
                    if p.get("type") in ("error", "assert"):
                        texts = " ".join(
                            str(a.get("description") or a.get("value"))[:120]
                            for a in p.get("args", []))
                        # harness pages log app state; only real errors count
                        errors.append("ERR " + texts[:200])
            status = "CLEAN" if not errors else f"{len(errors)} PROBLEM(S)"
            if errors:
                bad += 1
                print(f"{out}: {status}")
                for e in errors[:6]:
                    print("   ", e)
            else:
                print(f"{out}: CLEAN")
        print("pages with problems:", bad)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


def ws_recv_msg(ws, timeout=5):
    import shots_cdp
    return shots_cdp.ws_recv(ws.s, timeout=timeout)


if __name__ == "__main__":
    main()
