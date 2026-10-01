#!/usr/bin/env python3
"""CDP probe for the Halftone UI harness: loads one scenario, dumps DOM/CSS state.

Usage: python3 probe_cdp.py "<url-suffix>" "<js-expression>"
Prints the JSON result of the JS expression evaluated in the page.
Stdlib only. Reuses the ws helpers from shots_cdp.py.
"""
import base64, json, os, subprocess, sys, time, urllib.request
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shots_cdp as S

CHROME = S.CHROME
PORT = 9333

def main():
    suffix = sys.argv[1]
    expr = sys.argv[2]
    out_png = sys.argv[3] if len(sys.argv) > 3 else None
    wxh = sys.argv[4] if len(sys.argv) > 4 else "1200,760"
    w, h = (int(x) for x in wxh.split(","))
    proc = subprocess.Popen(
        [CHROME, "--headless=new", "--no-sandbox", "--disable-gpu",
         f"--remote-debugging-port={PORT}", f"--window-size={w},{h + 140}",
         "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                S.find_target(); break
            except Exception:
                time.sleep(0.3)
        else:
            raise RuntimeError("chrome did not come up")
        tgt = S.find_target()
        ws = S.Ws(tgt["webSocketDebuggerUrl"])
        ws.cmd("Page.enable"); ws.cmd("Runtime.enable")
        ws.cmd("Emulation.setDeviceMetricsOverride", width=w, height=h,
               deviceScaleFactor=1, mobile=False)
        ws.cmd("Page.navigate", url=f"{S.BASE}/test/ui_harness.html?{suffix}")
        time.sleep(1.2)
        ws.drain(3.0)
        time.sleep(2.0)
        r = ws.cmd("Runtime.evaluate", expression=expr, returnByValue=True,
                   awaitPromise=True)
        val = r.get("result", {}).get("value")
        exc = r.get("exceptionDetails")
        if exc:
            print("EXCEPTION:", json.dumps(exc)[:2000])
        else:
            print(json.dumps(val, indent=1)[:6000])
        if out_png:
            shot = ws.cmd("Page.captureScreenshot", format="png")
            with open(out_png, "wb") as f:
                f.write(base64.b64decode(shot["data"]))
            print("shot ->", out_png)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()

if __name__ == "__main__":
    main()
