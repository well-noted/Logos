#!/usr/bin/env python3
"""A stand-in for the Logos JVM bridge, for working on the desktop app without NetLogo.

It speaks protocol 0 (protocol/PROTOCOL.md) well enough for the app's screens: handshake,
opening a model (the view's size is read from the file), commands, observe, runs with view
frames, agent state and observations, stop, dispose and shutdown. It does not run NetLogo
code: `setup` resets a pretend world of moving turtles, anything containing `go` advances it
by one tick, `print`/`show` echo their argument, and reporters read pretend values. It is a
development tool only; no test of NetLogo behavior may rely on it.

Use it by pointing the bridge client at it (see app/README.md):

    LOGOS_JAVA=app/dev/mock-bridge.py  LOGOS_NETLOGO_HOME=<folder with app/netlogo-7.0.4.jar>
    LOGOS_BRIDGE_JAR=<any file>
"""

import json
import math
import re
import struct
import sys
import threading
import time
import xml.etree.ElementTree as ET

out_lock = threading.Lock()
stdout = sys.stdout.buffer
stdin = sys.stdin.buffer


def send(kind, payload):
    with out_lock:
        stdout.write(struct.pack("<IB", len(payload) + 1, kind) + payload)
        stdout.flush()


def send_json(obj):
    send(1, json.dumps(obj).encode("utf-8"))


def reply(id_, result=None, error=None):
    msg = {"jsonrpc": "2.0", "id": id_}
    if error is not None:
        msg["error"] = error
    else:
        msg["result"] = result if result is not None else {}
    send_json(msg)


def notify(method, params):
    send_json({"jsonrpc": "2.0", "method": method, "params": params})


class Workspace:
    def __init__(self, wid, dims):
        self.id = wid
        self.min_px, self.max_px, self.min_py, self.max_py, self.patch_size = dims
        self.ticks = None
        self.globals = {}
        self.turtles = []
        self.run = None
        self.run_count = 0
        self.lock = threading.Lock()

    def setup(self):
        self.ticks = 0.0
        w = self.max_px - self.min_px + 1
        h = self.max_py - self.min_py + 1
        self.turtles = []
        for i in range(60):
            self.turtles.append([float(i), self.min_px + (i * 7.3) % w, self.min_py + (i * 3.1) % h, (i * 37) % 360])

    def go(self):
        if self.ticks is None:
            return
        self.ticks += 1
        w = self.max_px - self.min_px + 1
        h = self.max_py - self.min_py + 1
        for t in self.turtles:
            t[3] = (t[3] + 13 * math.sin(self.ticks / 7 + t[0])) % 360
            t[1] = self.min_px + ((t[1] - self.min_px + 0.4 * math.sin(math.radians(t[3]))) % w)
            t[2] = self.min_py + ((t[2] - self.min_py + 0.4 * math.cos(math.radians(t[3]))) % h)

    def view_frame(self, run, seq):
        pw = self.max_px - self.min_px + 1
        ph = self.max_py - self.min_py + 1
        s = max(1, int(round(self.patch_size)))
        width, height = pw * s, ph * s
        tk = self.ticks or 0
        row_colors = []
        pix = bytearray(width * height * 4)
        # Patches: a slow-moving pattern.
        for py in range(ph):
            for px in range(pw):
                g = int(60 + 40 * math.sin((px + tk) / 9) + 40 * math.cos((py - tk) / 11))
                color = (30, max(0, min(255, g + 60)), 40, 255)
                for yy in range(s):
                    base = ((py * s + yy) * width + px * s) * 4
                    pix[base:base + 4 * s] = bytes(color) * s
        # Turtles: small yellow squares.
        for t in self.turtles:
            cx = int((t[1] - self.min_px + 0.5) * s)
            cy = int((self.max_py - t[2] + 0.5) * s)
            for dy in range(-2, 3):
                for dx in range(-2, 3):
                    x, y = cx + dx, cy + dy
                    if 0 <= x < width and 0 <= y < height:
                        i = (y * width + x) * 4
                        pix[i:i + 4] = bytes((240, 220, 60, 255))
        ticks = float("nan") if self.ticks is None else self.ticks
        head = struct.pack("<BIIQd", 1, self.id, run, seq, ticks)
        return head + struct.pack("<II", width, height) + bytes(pix)

    def agent_frame(self, run, seq):
        pw = self.max_px - self.min_px + 1
        ph = self.max_py - self.min_py + 1
        n = len(self.turtles)
        ticks = float("nan") if self.ticks is None else self.ticks
        b = struct.pack("<BIIQd", 2, self.id, run, seq, ticks)
        b += struct.pack("<Iiiii", pw * ph, self.min_px, self.max_px, self.min_py, self.max_py)
        b += bytes((30, 120, 40, 255)) * (pw * ph)
        b += struct.pack("<I", n)
        cols = [[t[0] for t in self.turtles], [t[1] for t in self.turtles], [t[2] for t in self.turtles], [t[3] for t in self.turtles]]
        for col in cols:
            b += struct.pack(f"<{n}d", *col)
        b += bytes((240, 220, 60, 255)) * n
        b += struct.pack(f"<{n}d", *([1.0] * n))
        b += struct.pack(f"<{n}H", *([0] * n))
        b += struct.pack(f"<{n}H", *([1 if i % 3 == 0 else 0 for i in range(n)]))
        b += bytes(n)
        return b

    def report(self, r):
        r = r.strip()
        low = r.lower()
        if low == "ticks":
            if self.ticks is None:
                return None, {"code": 1002, "message": "The tick counter has not been started yet. Use RESET-TICKS."}
            return self.ticks, None
        if low in self.globals:
            return self.globals[low], None
        if low.startswith("count"):
            return float(len(self.turtles)), None
        m = re.match(r"\[\s*([\w\-?]+)\s*\]\s+of\s+turtle\s+(\d+)", low)
        if m:
            who = int(m.group(2))
            if who >= len(self.turtles):
                return {"nobody": True}, None
            t = self.turtles[who]
            var = m.group(1)
            values = {"who": t[0], "xcor": t[1], "ycor": t[2], "heading": t[3], "color": 45.0, "size": 1.0,
                      "shape": "default", "label": "", "label-color": 9.9, "breed": {"agentset": "turtles", "count": len(self.turtles)},
                      "hidden?": False, "pen-size": 1.0, "pen-mode": "up"}
            if var in values:
                return values[var], None
            return None, {"code": 1002, "message": f"TURTLES breed does not own variable {var.upper()}"}
        if low in ("true", "false"):
            return low == "true", None
        if re.match(r"^-?\d+(\.\d+)?$", low):
            return float(low), None
        # Anything else: a pretend number that changes with ticks.
        tk = self.ticks or 0
        return round(50 + 30 * math.sin(tk / 15 + len(r)), 4), None


workspaces = {}
next_ws = [1]
model_paths = {}
experiments = {}
next_exp = [1]


def load_experiment(ws, p):
    """Reads an experiment from the model's XML and lists its runs in BehaviorSpace order."""
    root = ET.parse(model_paths[ws.id]).getroot()
    for e in root.iter("experiment"):
        if e.attrib.get("name") != p["name"]:
            continue
        variables = []
        for v in e.iter("enumeratedValueSet"):
            variables.append((v.attrib["variable"], [x.attrib["value"] for x in v.iter("value")]))
        for v in e.iter("steppedValueSet"):
            first, step, last = (float(v.attrib[k]) for k in ("first", "step", "last"))
            vals, x = [], first
            while x <= last + 1e-9:
                vals.append(repr(round(x, 10)).rstrip("0").rstrip(".") if x != int(x) else str(int(x)))
                x += step
            variables.append((v.attrib["variable"], vals))
        reps = int(e.attrib.get("repetitions", "1"))
        seq = e.attrib.get("sequentialRunOrder", "true") == "true"
        import itertools
        combos = list(itertools.product(*[vals for _, vals in variables])) if variables else [()]
        runs = []
        if seq:
            for c in combos:
                runs.extend([c] * reps)
        else:
            for _ in range(reps):
                runs.extend(combos)
        cell = lambda t: {"value": float(t) if re.match(r"^-?\d+(\.\d+)?$", t) else t.strip('"'), "text": t.strip('"')}
        eid = next_exp[0]
        next_exp[0] += 1
        metrics = [m.text or "" for m in e.iter("metric")]
        experiments[eid] = {"metrics": metrics, "every": e.attrib.get("runMetricsEveryStep") == "true", "runs": runs, "names": [n for n, _ in variables]}
        return {"experiment": eid, "name": p["name"], "repetitions": reps, "sequentialRunOrder": seq,
                "runMetricsEveryStep": e.attrib.get("runMetricsEveryStep") == "true", "runMetricsCondition": "",
                "timeLimit": int(e.attrib.get("timeLimit", "0")), "exitCondition": (e.findtext("exitCondition") or ""),
                "preExperiment": "", "setup": e.findtext("setup") or "", "go": e.findtext("go") or "", "postRun": "",
                "postExperiment": "", "metrics": metrics, "variables": [n for n, _ in variables],
                "runs": [[dict(name=n, **cell(v)) for n, v in zip([n for n, _ in variables], c)] for c in runs],
                "worldDimensions": {"minPxcor": ws.min_px, "maxPxcor": ws.max_px, "minPycor": ws.min_py, "maxPycor": ws.max_py}}
    return None


def run_experiment(ws, p):
    e = experiments[p["experiment"]]
    n = p["runNumber"]
    steps = 15 + (n * 7) % 11
    time.sleep(0.4 + 0.05 * (n % 5))
    settings = [{"name": name, "value": v, "text": v} for name, v in zip(e["names"], e["runs"][n - 1])]
    def cells(step):
        return [{"value": round(10 + step * 0.5 + (n % 7) + i, 3), "text": str(round(10 + step * 0.5 + (n % 7) + i, 3))} for i in range(len(e["metrics"]))]
    measurements = [{"step": s, "values": cells(s)} for s in (range(steps + 1) if e["every"] else [steps])]
    return {"runNumber": n, "settings": settings, "reason": "completed", "steps": steps, "measurements": measurements,
            "errors": [], "elapsedMillis": 400.0}


def view_dims(path):
    try:
        root = ET.parse(path).getroot()
        v = root.find("widgets/view")
        if v is not None:
            a = v.attrib
            return (int(a.get("minPxcor", -16)), int(a.get("maxPxcor", 16)), int(a.get("minPycor", -16)),
                    int(a.get("maxPycor", 16)), float(a.get("patchSize", 13)))
    except Exception:
        pass
    return (-16, 16, -16, 16, 13.0)


def run_loop(ws, run_id, source, repeat, stream):
    view = stream.get("view")
    agents = stream.get("agents")
    observe = stream.get("observe")
    seq = {"v": 0, "a": 0}
    last = {"v": 0.0, "a": 0.0}

    def emit(force=False):
        now = time.time()
        if view and (force or now - last["v"] >= 1.0 / view.get("maxFps", 30)):
            send(2, ws.view_frame(run_id, seq["v"]))
            seq["v"] += 1
            last["v"] = now
        if agents and (force or now - last["a"] >= 1.0 / agents.get("maxFps", 30)):
            send(2, ws.agent_frame(run_id, seq["a"]))
            seq["a"] += 1
            last["a"] = now

    started = time.time()
    emit(force=True)
    iterations = 0
    reason = "completed"
    while True:
        if ws.run["stop"]:
            reason = "halted"
            break
        if repeat is not None and iterations >= repeat:
            reason = "completed"
            break
        if source.strip():
            if "go" in source:
                ws.go()
            time.sleep(0.02)
        iterations += 1
        emit()
        if observe and observe.get("reporters"):
            vals = []
            for r in observe["reporters"]:
                v, err = ws.report(r)
                vals.append({"error": err} if err else {"ok": v})
            notify("run.observed", {"workspace": ws.id, "run": run_id, "ticks": ws.ticks, "values": vals})
        if ws.ticks is not None and ws.ticks >= 2000:
            reason = "stopped"
            break
    emit(force=True)
    with ws.lock:
        ws.run = None
    notify("run.finished", {"workspace": ws.id, "run": run_id, "reason": reason, "iterations": iterations,
                            "ticks": ws.ticks, "elapsedMillis": (time.time() - started) * 1000, "error": None})


def handle(msg):
    id_ = msg.get("id")
    method = msg.get("method")
    p = msg.get("params") or {}
    if method == "engine.hello":
        return reply(id_, {"protocolVersion": 0, "bridgeVersion": "mock-0", "netlogoVersion": "NetLogo 7.0.4 (mock bridge)",
                           "javaVersion": "none", "javaVendor": "mock-bridge.py", "extensionsDirs": [], "modelsDir": None})
    if method == "engine.shutdown":
        reply(id_, {})
        sys.exit(0)
    if method == "workspace.open":
        wid = next_ws[0]
        next_ws[0] += 1
        workspaces[wid] = Workspace(wid, view_dims(p["path"]))
        model_paths[wid] = p["path"]
        return reply(id_, {"workspace": wid, "modelVersion": "NetLogo 7.0.4", "compiled": True, "errors": []})
    ws = workspaces.get(p.get("workspace"))
    if method.startswith("workspace.") and ws is None:
        return reply(id_, error={"code": 1003, "message": "no such workspace"})
    if method == "workspace.command":
        src = p.get("source", "")
        if ws.run is not None:
            return reply(id_, error={"code": 1005, "message": "busy", "data": {"run": ws.run["id"]}})
        for line in src.splitlines() or [src]:
            low = line.strip().lower()
            m = re.match(r"set\s+([\w\-?]+)\s+(.*)", low)
            if m:
                val = m.group(2).strip()
                try:
                    ws.globals[m.group(1)] = float(val)
                except ValueError:
                    ws.globals[m.group(1)] = val.strip('"') if val not in ("true", "false") else val == "true"
            if "setup" in low or "clear-all" in low or "reset-ticks" in low:
                ws.setup()
            if re.search(r"\bgo\b", low):
                ws.go()
            m = re.match(r"(output-)?(print|show|type|write)\s+(.*)", line.strip())
            if m:
                value, err = ws.report(m.group(3))
                text = m.group(3) if err or value is None else (
                    int(value) if isinstance(value, float) and value.is_integer() else value)
                prefix = "observer: " if m.group(2) == "show" else ""
                notify("workspace.output", {"workspace": ws.id, "text": f"{prefix}{text}\n",
                                            "outputArea": bool(m.group(1))})
            if low == "error":
                return reply(id_, error={"code": 1001, "message": "Nothing named ERROR has been defined.", "data": {"span": {"start": 0, "end": 5, "file": None}}})
        return reply(id_, {})
    if method == "workspace.observe":
        vals = []
        for r in p.get("reporters", []):
            v, err = ws.report(r)
            vals.append({"error": err} if err else {"ok": v})
        return reply(id_, {"ticks": ws.ticks, "values": vals})
    if method == "workspace.run":
        with ws.lock:
            if ws.run is not None:
                return reply(id_, error={"code": 1005, "message": "busy", "data": {"run": ws.run["id"]}})
            ws.run_count += 1
            ws.run = {"id": ws.run_count, "stop": False}
            run_id = ws.run_count
        reply(id_, {"run": run_id})
        threading.Thread(target=run_loop, args=(ws, run_id, p.get("source", ""), p.get("repeat"), p.get("stream") or {}), daemon=True).start()
        return
    if method == "workspace.stop":
        with ws.lock:
            if ws.run is not None:
                ws.run["stop"] = True
        return reply(id_, {})
    if method == "workspace.dispose":
        with ws.lock:
            if ws.run is not None:
                ws.run["stop"] = True
        workspaces.pop(ws.id, None)
        return reply(id_, {})
    if method == "experiment.load":
        return reply(id_, load_experiment(ws, p))
    if method == "experiment.run":
        return reply(id_, run_experiment(ws, p))
    if method == "experiment.command":
        return reply(id_, {"error": None})
    if method == "experiment.close":
        return reply(id_, {})
    if method == "workspace.program":
        return reply(id_, {"globals": [], "interfaceGlobals": [], "turtlesOwn": [], "patchesOwn": [], "linksOwn": [], "breeds": [], "procedures": []})
    if method == "engine.primitives":
        # A few names of each class, so highlighting can be seen; not NetLogo's table.
        def prim(name, kind):
            return {"name": name, "kind": kind, "left": None, "args": [], "returns": None,
                    "defaultArgCount": 0, "minimumArgCount": 0, "variadic": False, "precedence": 0,
                    "isRightAssociative": False, "agentClass": "OTPL", "blockAgentClass": None,
                    "introducesContext": False, "canBeConcise": False, "extension": None}
        cmds = ["show", "print", "ask", "set", "let", "create-turtles", "crt", "clear-all", "ca",
                "reset-ticks", "tick", "if", "ifelse", "foreach", "repeat", "while", "die", "move-to",
                "fd", "rt", "lt", "setxy", "hatch", "report", "stop", "file-open", "file-close"]
        reps = ["count", "of", "with", "one-of", "random", "random-float", "mean", "max", "min",
                "sum", "item", "length", "list", "patch", "turtles", "patches", "links", "ticks",
                "any?", "neighbors", "distance", "and", "or", "not", "file-read", "round"]
        breed = [dict(prim("create-<breeds>", "command"), breedKind="turtle"),
                 dict(prim("<breeds>-here", "reporter"), breedKind="turtle")]
        return reply(id_, {"netlogoVersion": "NetLogo 7.0.4 (mock)",
                           "primitives": [prim(c, "command") for c in cmds] + [prim(r, "reporter") for r in reps],
                           "agentVariables": {"observer": [], "patch": [{"name": "PCOLOR", "type": {"name": "number", "mask": 1}}],
                                              "turtle": [{"name": n, "type": {"name": "number", "mask": 1}} for n in ("XCOR", "YCOR", "COLOR", "SIZE", "WHO")],
                                              "link": []},
                           "constants": [{"name": n, "value": True} for n in ("true", "false", "nobody", "red", "green", "blue")],
                           "keywords": ["to", "to-report", "end", "globals", "breed", "turtles-own", "patches-own", "extensions"],
                           "breedPrimitives": breed, "typeMasks": {}})
    if method == "workspace.check":
        return reply(id_, {"errors": []})
    return reply(id_, error={"code": -32601, "message": f"the mock bridge has no {method}"})


def main():
    while True:
        head = stdin.read(4)
        if len(head) < 4:
            return
        (length,) = struct.unpack("<I", head)
        body = stdin.read(length)
        if len(body) < length:
            return
        if body[0] != 1:
            continue
        handle(json.loads(body[1:].decode("utf-8")))


if __name__ == "__main__":
    main()
