import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

plugins {
    kotlin("jvm") version "2.4.10"
}

val externalRoot = file(providers.environmentVariable("TERA_KOTLIN_SMOKE_ROOT").get())
val projectOutput = file(providers.environmentVariable("EXT_BUILD_PROJECT_DIR").get())
require(!providers.environmentVariable("EXT_BUILD_RUN_ACTIVE").orNull.isNullOrBlank())
require(externalRoot.isAbsolute && !externalRoot.toPath().startsWith(rootDir.toPath()))
require(externalRoot.toPath().startsWith(projectOutput.toPath()))
layout.buildDirectory.set(externalRoot.resolve("build"))

kotlin {
    jvmToolchain(21)
    sourceSets.main { kotlin.srcDir(externalRoot.resolve("generated")) }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
    testImplementation(kotlin("test-junit"))
}

dependencyLocking {
    lockAllConfigurations()
    lockMode.set(LockMode.STRICT)
}

tasks.named<KotlinCompile>("compileTestKotlin") {
    compilerOptions.allWarningsAsErrors.set(true)
}

tasks.test {
    useJUnit()
    maxParallelForks = 1
    systemProperty("jna.library.path", providers.environmentVariable("TERA_KOTLIN_NATIVE_DIR").get())
    systemProperty("tera.smoke.data", externalRoot.resolve("test_data").absolutePath)
    testLogging { events("passed", "failed", "skipped") }
    // A smoke run must execute the native boundary even when compilation is cached.
    outputs.upToDateWhen { false }
}
