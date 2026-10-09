# Logos bridge

The JVM process that runs NetLogo's `HeadlessWorkspace` and talks to Logos over stdin and stdout. The protocol is defined in `../protocol/PROTOCOL.md`.

License: GPL-3.0-or-later (`LICENSE`). This folder is the only part of Logos that runs inside the same process as NetLogo, which is why it is licensed separately from the rest of the repository (`../docs/decisions.md`, D-01).

## Build and test

Needs JDK 17 or newer on `JAVA_HOME` and NetLogo 7.0.4 installed. The build compiles against the jars in the NetLogo install and the tests run models on it. Gradle looks for NetLogo in `-PnetlogoHome=...`, then `LOGOS_NETLOGO_HOME`, then the default install folder. The Gradle wrapper downloads Gradle itself on first use.

```
cd bridge
.\gradlew.bat test       # Windows
./gradlew test           # macOS, Linux
.\gradlew.bat jar        # builds build\libs\logos-bridge-0.0.0.jar
```

`WorkspaceTest` opens `corpus/own/aa4-combined/AA4-Combined.nlogox` and small models built in the test, and drives them through the protocol the same way the Rust client will.

## Running by hand

The client normally starts the bridge. To start it yourself with NetLogo 7.0.4's bundled Java runtime:

```
& "C:\Program Files\NetLogo 7.0.4\runtime\bin\java.exe" `
  -cp "build\libs\logos-bridge-0.0.0.jar;C:\Program Files\NetLogo 7.0.4\app\netlogo-7.0.4.jar" `
  org.logos.bridge.Main
```

It then waits for framed messages on stdin, so there is nothing useful to type into it. Close stdin (Ctrl+Z, Enter) and it exits.

## Layout

| File | Purpose |
|---|---|
| `Main.java` | Process entry point. Reserves stdout for protocol frames and sends everything else to stderr. |
| `Bridge.java` | Request loop and dispatch. |
| `Frames.java` | Frame encoding (PROTOCOL.md section 2). |
| `Json.java` | Dependency-free JSON reader and writer with exact double formatting. |
| `RpcException.java` | Error codes (PROTOCOL.md section 8). |
| `Workspaces.java` | Workspace registry, one job thread per workspace, `workspace.*` methods. |
| `Values.java` | NetLogo values to JSON (PROTOCOL.md section 7). |
| `ViewFrame.java` | Encodes NetLogo's rendered view as a binary view frame (PROTOCOL.md section 6.1). |
