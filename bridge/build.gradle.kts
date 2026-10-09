// Logos JVM bridge. GPL-3.0-or-later (see LICENSE and docs/decisions.md D-01).

plugins {
    java
}

group = "org.logos"
version = "0.0.0"

repositories {
    mavenCentral()
}

tasks.withType<JavaCompile>().configureEach {
    // Runs on the Java 17 runtime bundled with NetLogo 7.0.4 (D-05).
    // Any JDK 17 or newer can build it.
    options.release = 17
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))
}

dependencies {
    testImplementation(platform("org.junit:junit-bom:5.11.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.test {
    useJUnitPlatform()
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
