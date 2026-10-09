# Logos bridge protocol

Version: **0** (Phase 0, unstable). Any change to this file changes the protocol for both sides. A PR that changes it also updates `crates/logos-protocol` and `bridge/` in the same PR.

This document defines how a Logos process (the **client**, written in Rust) talks to the JVM bridge (the **bridge**, written in Java, running NetLogo's `HeadlessWorkspace`).

## 1. Process model

- The client starts the bridge as a child process using NetLogo's bundled Java runtime:

  ```
  <install>\runtime\bin\java.exe <JVM options> -cp "<bridge.jar>;<install>\app\netlogo-<version>.jar" org.logos.bridge.Main
  ```

  The JVM options are listed in `docs/repo-layout.md` under "Finding NetLogo". The classpath separator is `;` on Windows and `:` elsewhere.
- One bridge process serves one NetLogo version and hosts any number of workspaces (D-09).
- **stdin** carries messages from client to bridge. **stdout** carries messages from bridge to client. Both are binary streams of frames (section 2). Nothing else may be written to stdout. NetLogo output that would normally go to stdout (`print`, `show`, `type`, `write`) is captured and sent as `workspace.output` notifications.
- **stderr** carries human-readable log lines only. The client copies them to its log. The client never parses stderr.
- When the bridge reads end-of-file on stdin, it disposes all workspaces and exits within 2 seconds. This is how the bridge exits when the client crashes or is killed (Phase 0 criterion A10).

## 2. Framing

Every message is one frame:

| Bytes | Field |
|---|---|
| 4 | `length`: unsigned 32-bit little-endian. Number of bytes after this field. |
| 1 | `kind`: `0x01` = JSON message, `0x02` = binary data |
| `length - 1` | payload |

- Maximum frame size is 256 MiB. A receiver that reads a larger `length` treats the stream as corrupt and closes it.
- `kind = 0x01`: the payload is one UTF-8 JSON object (section 3).
- `kind = 0x02`: the payload is a binary data frame (section 6).

## 3. JSON messages

JSON messages use the JSON-RPC 2.0 shapes: request, response, notification.

- **Requests** go from client to bridge only. `id` is a positive integer chosen by the client, unique among requests in flight.
- **Responses** go from bridge to client and carry the request's `id`. The bridge may answer requests for different workspaces in any order.
- **Notifications** have no `id`. They go from bridge to client only.
- Field names are `camelCase`.

Example:

```json
{"jsonrpc":"2.0","id":7,"method":"workspace.command","params":{"workspace":1,"source":"setup"}}
{"jsonrpc":"2.0","id":7,"result":{}}
```

### 3.1 Handshake

The first request must be `engine.hello`. The bridge rejects every other method with error `1000 NotInitialized` until the handshake succeeds.

## 4. Threading and ordering

- Each workspace has one job thread owned by the bridge. Every request that touches a workspace runs on that thread, in the order the requests arrived.
- While a workspace is running (`workspace.run`), it accepts only `workspace.stop`, `workspace.observe` and `workspace.dispose`. Other requests for that workspace fail with `1005 Busy`. `workspace.observe` during a run is answered between two iterations of the run command.
- Requests for different workspaces run in parallel.
- `engine.*` and `model.*` requests run on a separate bridge thread and never wait for a workspace.

## 5. Methods

Types used below: `WorkspaceId` is a positive integer. `Value` is defined in section 7. `SourceSpan` is `{ "start": int, "end": int, "file": string | null }`, character offsets into the source that failed. When `file` is null, the offsets refer to the source string sent in the request (for `workspace.command` and `workspace.observe`) or to the model's code section (for `workspace.open`). Otherwise `file` names the `__includes` file. NetLogo compiles typed commands and reporters inside a wrapper, and the bridge removes the wrapper's length from the positions it reports.

### 5.1 Engine

**`engine.hello`**

```
params: { "protocolVersion": 0, "client": string }
result: { "protocolVersion": 0, "bridgeVersion": string, "netlogoVersion": string | null,
          "javaVersion": string, "javaVendor": string,
          "extensionsDirs": [string], "modelsDir": string }
```

If `protocolVersion` differs from the bridge's, the bridge returns error `1006 ProtocolMismatch` and exits.

`netlogoVersion` is read from the NetLogo jar's manifest. It is null only when NetLogo is not on the classpath, which happens in the bridge's own unit tests and never in normal use. `extensionsDirs` and `modelsDir` come from the `netlogo.extensions.dir` and `netlogo.models.dir` system properties; `extensionsDirs` lists the user extensions folder and its `.bundled` subfolder.

**`engine.primitives`**

Returns the primitive table that `logos-semantic` uses (D-01, D-07): the syntax of every core primitive and of the primitives of the requested extensions, as NetLogo 7.0.4 defines them.

```
params: { "extensions": [string] }          // extensions to load first, e.g. ["nw", "table"]
result: { "netlogoVersion": string,          // "NetLogo 7.0.4"
          "primitives": [ {
              "name": string,                  // lower case: "fd", "nw:generate-random"
              "kind": "command" | "reporter",
              "left": ArgType | null,          // the left argument of an infix reporter
              "args": [ArgType],               // arguments on the right, in order
              "returns": ArgType | null,       // null for commands
              "defaultArgCount": int,          // NetLogo's Syntax.dfault
              "minimumArgCount": int,          // NetLogo's Syntax.minimum
              "variadic": bool,                // takes a variable number of arguments in parentheses
              "precedence": int,
              "isRightAssociative": bool,
              "agentClass": string,            // e.g. "OTPL", "-T--", as NetLogo writes it
              "blockAgentClass": string | null,
              "introducesContext": bool,
              "extension": string | null } ] } // e.g. "nw"; null for core primitives

ArgType: { "name": string, "mask": int }
```

`mask` is NetLogo's type bit mask (`org.nlogo.core.Syntax`), which includes the repeatable and optional flags. `name` is NetLogo's own name for the mask (`org.nlogo.core.TypeNames.name`), as its error messages show it, for example `"agent or agentset"` or `"command block (optional)"`. The bridge passes both through without translating.

Core primitives are the names `org.nlogo.parse.TokenMapper` knows, including the internal `__` primitives: 459 in NetLogo 7.0.4. Breed primitives such as `create-<breeds>` are templates in NetLogo and are not listed. Extension primitives are read after a temporary workspace with `extensions [ ... ]` has compiled. An extension NetLogo can't find fails with `-32602` and NetLogo's message. The rows are sorted by name.

**`engine.shutdown`**

```
params: {}
result: {}
```

The bridge disposes every workspace, sends the response, and exits with code 0.

### 5.2 Workspaces

**`workspace.open`**

Creates a workspace and opens a model file in it. Use a file path, not a string, for any model that reads files relative to its own folder (`file-open`, `import-*`, `__includes`); NetLogo sets the current directory from the path.

```
params: { "path": string }
result: { "workspace": WorkspaceId,
          "modelVersion": string,               // the model file's version attribute
          "compiled": bool,
          "errors": [ { "message": string, "span": SourceSpan } ] }
```

A model that fails to compile still produces a workspace (so the editor can show the errors). `compiled` is false and `errors` lists the problems. `modelVersion` is the `version` attribute of the file's `<model>` element.

A file NetLogo can't read (missing, unreadable, or written by a newer NetLogo version) fails with `1004 ModelOpenFailed` and creates no workspace.

**`workspace.openString`**

Same result as `workspace.open`, for tests and for models with no file dependencies.

```
params: { "contents": string, "format": "nlogox" }
```

**`workspace.command`**

Runs NetLogo commands as the observer. This is the only method that may advance the model's main random number generator (`world.mainRNG`).

```
params: { "workspace": WorkspaceId, "source": string }
result: {}
```

Errors: `1001 CompilerError`, `1002 RuntimeError`.

**`workspace.observe`**

Evaluates reporters without affecting the run (D-14). Each reporter runs with a job owner holding a clone of `world.mainRNG`, the same way BehaviorSpace evaluates metrics. Reporters that change model state (`[set x 0] of turtles` and similar) are not prevented by the bridge; avoiding them is the caller's responsibility.

```
params: { "workspace": WorkspaceId, "reporters": [string] }
result: { "ticks": number | null,              // null before reset-ticks
          "values": [ { "ok": Value } | { "error": ErrorData } ] }
```

The reporters are compiled once per distinct string and the compiled form is cached per workspace. The result has one entry per reporter, in order. One failing reporter does not fail the others.

There is deliberately no `workspace.report` method. Reading a value through `HeadlessWorkspace.report` uses the main random number generator and can change the run.

**`workspace.run`**

Runs a command repeatedly, the way a forever button does, and returns immediately. The run continues in the background until one of: the command executes `stop` at the top level of the procedure (NetLogo's forever-button rule, detected the same way BehaviorSpace detects it), `repeat` iterations have completed, `workspace.stop` is called, or a runtime error occurs.

```
params: { "workspace": WorkspaceId,
          "source": string,                    // usually "go"
          "repeat": int | null,                // null = until stopped
          "stream": {
             "view": { "everyTicks": int, "maxFps": number } | null,
             "agents": { "everyTicks": int, "maxFps": number } | null,
             "observe": { "reporters": [string], "everyTicks": int } | null } }
result: { "run": int }
```

- `view`: send a view frame (section 6.1) after an iteration when at least `everyTicks` ticks have passed since the last frame and at least `1/maxFps` seconds have passed. Frames are rendered on the job thread between iterations. The first frame (sequence 0) shows the state before the first iteration, and a final frame is always sent when the run ends, so the client sees the end state. `everyTicks` defaults to 1 and `maxFps` to 30.
- `agents`: send an agent-state frame (section 6.2) under the same rule, with the same first and final frames.
- `observe`: send a `run.observed` notification every `everyTicks` ticks, with values computed as in `workspace.observe`.

For models that don't use ticks, "ticks passed" counts iterations instead. View frames and agent frames of one run each have their own sequence numbers, starting at 0.

The command is compiled as `source` followed by NetLogo's `__experimentstepend`, the same way BehaviorSpace compiles its go commands (`lab/Worker.scala` at `v7.0.4`). That is how a `stop` at the top level of the procedure is detected. Each iteration runs on the model's main random number generator, so a run gives the same results as the same commands sent through `workspace.command`. A compile error is returned as the response, and no run starts. The response `{ "run": int }` is sent once the command has compiled, before the first iteration.

**`workspace.stop`**

Asks the run to stop after the current iteration, as pressing a forever button a second time does in NetLogo. The current iteration is not interrupted. The run then ends with a `run.finished` notification whose `reason` is `"halted"`. Calling it when nothing is running does nothing. `workspace.dispose` during a run stops it the same way before disposing.

```
params: { "workspace": WorkspaceId }
result: {}
```

**`workspace.exportWorld`**

```
params: { "workspace": WorkspaceId, "path": string }
result: {}
```

**`workspace.dispose`**

Stops any run, disposes the workspace and frees its memory.

```
params: { "workspace": WorkspaceId }
result: {}
```

### 5.3 Models

**`model.convertLegacy`**

Converts a `.nlogo` file to `.nlogox` the way NetLogo 7.0.4's GUI does when it opens an older model and the user saves it (D-06), using NetLogo's own reader, auto-converter and writer. Code written for older versions is rewritten by NetLogo's auto-conversion (for example NetLogo 5's `?` in `foreach` becomes `[ ?1 -> ... ]`), and the version is set to `NetLogo 7.0.4`. Never writes to `sourcePath`.

```
params: { "sourcePath": string, "outputPath": string }   // outputPath must end in .nlogox
result: { "outputPath": string,
          "sourceVersion": string,          // the source's version line, e.g. "NetLogo 5.0.3"
          "warnings": [string] }            // auto-conversion problems; the GUI shows the same
```

- Fails with `1007 FileExists` if `outputPath` exists, including one created while the conversion runs; the file is created with create-new semantics.
- Fails with `1004 ModelOpenFailed` if the source can't be read or isn't a NetLogo model.
- `.nlogo3d` files are not supported yet (`-32602`).
- When auto-conversion fails for part of the model, the GUI shows the errors and, if the user continues, opens the partly converted model. This method does the same: it writes the partly converted model and returns the errors as `warnings`.

### 5.4 Notifications (bridge to client)

| Method | Params |
|---|---|
| `workspace.output` | `{ "workspace": WorkspaceId, "text": string }` — text printed by the model |
| `run.observed` | `{ "workspace": WorkspaceId, "run": int, "ticks": number \| null, "values": [ {"ok": Value} \| {"error": ErrorData} ] }` |
| `run.finished` | `{ "workspace": WorkspaceId, "run": int, "reason": "stopped" \| "completed" \| "halted" \| "error", "iterations": int, "ticks": number \| null, "elapsedMillis": number, "error": ErrorData \| null }` |

`reason` values: `"stopped"` = the command executed `stop`; `"completed"` = `repeat` iterations finished; `"halted"` = `workspace.stop`, `workspace.dispose` or bridge shutdown; `"error"` = runtime error. `iterations` counts completed iterations; an iteration that fails with a runtime error is not counted.

## 6. Binary data frames

A binary frame (`kind = 0x02`) starts with a common header. All integers are little-endian.

| Bytes | Field |
|---|---|
| 1 | `type`: `0x01` = view frame, `0x02` = agent state |
| 4 | `workspace` (u32) |
| 4 | `run` (u32) |
| 8 | `sequence` (u64), increasing per run and frame type, starting at 0 |
| 8 | `ticks` (f64); NaN when ticks have not been reset |

### 6.1 View frame (`type = 0x01`)

| Bytes | Field |
|---|---|
| 4 | `width` in pixels (u32) |
| 4 | `height` in pixels (u32) |
| `width × height × 4` | pixels, row-major from the top-left, each pixel R, G, B, A (one byte each) |

The image is NetLogo's own rendering of the view (`HeadlessWorkspace.exportView`), so it matches what the NetLogo GUI shows.

### 6.2 Agent state (`type = 0x02`)

Phase 0 layout, for the A-state throughput measurement. It will change in Phase 1.

| Bytes | Field |
|---|---|
| 4 | `patchCount` (u32) |
| 4 | `minPxcor` (i32) |
| 4 | `maxPxcor` (i32) |
| 4 | `minPycor` (i32) |
| 4 | `maxPycor` (i32) |
| `patchCount × 4` | patch colors as RGBA, row by row from the top-left patch (`minPxcor`, `maxPycor`). Alpha is always 255: NetLogo draws patches opaque. |
| 4 | `turtleCount` (u32) |
| then one column after another, each `turtleCount` long: | |
| `× 8` | `who` (f64) |
| `× 8` | `xcor` (f64) |
| `× 8` | `ycor` (f64) |
| `× 8` | `heading` (f64) |
| `× 4` | `color` as RGBA |
| `× 8` | `size` (f64) |
| `× 2` | `shapeIndex` (u16), index into the model's turtle shapes in NetLogo's shape list order: `default` first, then the others sorted by name |
| `× 2` | `breedIndex` (u16), 0 = `turtles`, then breeds in declaration order |
| `× 1` | `hidden` (u8, 0 or 1) |

Agent state is read on the job thread between iterations. Reading it does not use the random number generator: patch colors come from the array NetLogo's renderer uses (`World.patchColors`), and turtles are read in `who` order with `TreeAgentSet.iterator`, not with a shuffling agentset iteration.

Size: 25 (common header) + 20 + 4 × patches + 4 + 49 × turtles bytes. With 10,201 patches and 10,000 turtles, 530,853 bytes (518 KiB).

## 7. Values

NetLogo values are encoded in JSON as:

| NetLogo | JSON |
|---|---|
| number | JSON number. The bridge writes doubles with Java's `Double.toString` round-trip form, so the client parses the exact same 64-bit value. |
| string | JSON string |
| boolean | `true` / `false` |
| list | JSON array of values |
| `nobody` | `{"nobody": true}` |
| turtle | `{"turtle": who}` |
| patch | `{"patch": [pxcor, pycor]}` |
| link | `{"link": [end1Who, end2Who], "breed": string}` |
| agentset | `{"agentset": "turtles" \| "patches" \| "links" \| "observer", "count": int}` |
| anonymous procedure | `{"procedure": string}` (source text) |
| extension object (for example a `table`) | `{"extension": string, "display": string}` (the value as `show` would print it) |
| anything else | `{"display": string}` |

A turtle or link that has died is encoded as `nobody`, as NetLogo reports it.

## 8. Errors

Errors use the JSON-RPC `error` object: `{ "code": int, "message": string, "data": ErrorData }`. Standard JSON-RPC codes (-32700, -32600, -32601, -32602, -32603) keep their usual meaning.

| Code | Name | `data` |
|---|---|---|
| 1000 | NotInitialized | — |
| 1001 | CompilerError | `{ "span": SourceSpan }` |
| 1002 | RuntimeError | `{ "agent": string \| null, "procedure": string \| null, "netlogoStack": string }` |
| 1003 | NoSuchWorkspace | — |
| 1004 | ModelOpenFailed | `{ "path": string, "cause": string }` |
| 1005 | Busy | `{ "run": int }` |
| 1006 | ProtocolMismatch | `{ "bridge": int, "client": int }` |
| 1007 | FileExists | `{ "path": string }` |
| 1099 | Internal | `{ "javaStack": string }` |

`ErrorData` in `workspace.observe` and notifications is `{ "code": int, "message": string, ...the data fields above }`.

A `RuntimeError` or `CompilerError` never ends the bridge process. The workspace stays usable.

## 9. Open points for Phase 0

- Settled: `HeadlessWorkspace.exportView` is called on the job thread between iterations. `RunTest.runMatchesTheSameStepsAsCommands` checks that a run with frames gives the same model state as the same steps without them.
- Settled: top-level `stop` is detected with `__experimentstepend`, as in BehaviorSpace.
- Confirm which random generator plot update code uses in a headless run (A12).
- Decide whether `workspace.output` should be buffered per iteration to avoid one notification per `print`.
