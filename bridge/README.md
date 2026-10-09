# Logos bridge

The JVM process that runs NetLogo's `HeadlessWorkspace` and talks to Logos over stdin and stdout. The protocol is defined in `../protocol/PROTOCOL.md`.

License: GPL-3.0-or-later (`LICENSE`). This folder is the only part of Logos that runs inside the same process as NetLogo, which is why it is licensed separately from the rest of the repository (`../docs/decisions.md`, D-01).

## Build and test

Needs JDK 17 or newer on `JAVA_HOME`. The Gradle wrapper downloads Gradle itself on first use.

```
cd bridge
.\gradlew.bat test       # Windows
./gradlew test           # macOS, Linux
.\gradlew.bat jar        # builds build\libs\logos-bridge-0.0.0.jar
```

The unit tests don't need NetLogo. Tests that run against an installed NetLogo arrive with the workspace methods.

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
