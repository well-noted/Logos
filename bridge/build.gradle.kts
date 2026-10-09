// Logos JVM bridge. GPL-3.0-or-later (see LICENSE and docs/decisions.md D-01).

plugins {
    java
}

group = "org.logos"
version = "0.0.0"

repositories {
    mavenCentral()
}

// ---------------------------------------------------------------- NetLogo

// The bridge compiles and tests against an installed NetLogo (docs/repo-layout.md,
// "Finding NetLogo"). Search order: -PnetlogoHome=..., LOGOS_NETLOGO_HOME, default locations.
val netlogoVersion = "7.0.4"

fun isNetlogoHome(dir: File) = dir.resolve("app/netlogo-$netlogoVersion.jar").isFile

val netlogoHome: File = run {
    val explicit = providers.gradleProperty("netlogoHome").orNull
        ?: System.getenv("LOGOS_NETLOGO_HOME")
    val candidates = if (explicit != null) {
        listOf(file(explicit))
    } else {
        listOfNotNull(
            System.getenv("ProgramFiles")?.let { file("$it/NetLogo $netlogoVersion") },
            file("C:/Program Files/NetLogo $netlogoVersion"),
            file("/Applications/NetLogo $netlogoVersion"),
            file("${System.getProperty("user.home")}/NetLogo $netlogoVersion"),
            file("/opt/NetLogo $netlogoVersion"),
        )
    }
    candidates.firstOrNull(::isNetlogoHome) ?: throw GradleException(
        "NetLogo $netlogoVersion not found (looked in ${candidates.joinToString()}). " +
            "Set LOGOS_NETLOGO_HOME or pass -PnetlogoHome=<install folder>."
    )
}

val netlogoJars = fileTree(netlogoHome.resolve("app")) { include("*.jar") }

dependencies {
    // Provided at run time by the NetLogo install; never bundled into the bridge jar.
    compileOnly(netlogoJars)
    testImplementation(netlogoJars)

    testImplementation(platform("org.junit:junit-bom:5.11.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

// ---------------------------------------------------------------- compile

tasks.withType<JavaCompile>().configureEach {
    // Runs on the Java 17 runtime bundled with NetLogo 7.0.4 (D-05).
    // Any JDK 17 or newer can build it.
    options.release = 17
    options.encoding = "UTF-8"
    // "path" and "classfile" warnings come from NetLogo's jars (manifest Class-Path entries and
    // Scala class files), not from bridge code.
    options.compilerArgs.addAll(listOf("-Xlint:all,-path,-classfile,-processing", "-Werror", "-proc:none"))
}

// ---------------------------------------------------------------- test

tasks.test {
    useJUnitPlatform()
    // The same JVM options NetLogo's own launcher uses (docs/repo-layout.md).
    jvmArgs(
        "-Djava.awt.headless=true",
        "-Dfile.encoding=UTF-8",
        "--add-exports=java.base/java.lang=ALL-UNNAMED",
        "--add-exports=java.desktop/sun.awt=ALL-UNNAMED",
        "--add-exports=java.desktop/sun.java2d=ALL-UNNAMED",
    )
    systemProperty("netlogo.extensions.dir", netlogoHome.resolve("extensions").path)
    systemProperty("netlogo.models.dir", netlogoHome.resolve("models").path)
    systemProperty("netlogo.docs.dir", netlogoHome.path)
    systemProperty("logos.netlogoVersion", netlogoVersion)
    systemProperty("logos.corpus", rootDir.resolve("../corpus").canonicalPath)
    maxHeapSize = "2g"
    testLogging {
        events("failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

tasks.jar {
    manifest {
        attributes(
            "Main-Class" to "org.logos.bridge.Main",
            "Implementation-Title" to "Logos bridge",
            "Implementation-Version" to project.version,
        )
    }
}
