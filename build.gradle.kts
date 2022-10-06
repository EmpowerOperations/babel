
plugins {
    java
    kotlin("jvm") version "1.6.21"
}

val antlrVersion = "4.8-1"

group = "com.empowerops"
version = "0.19"

repositories {
    mavenCentral()
}

dependencies {

    implementation(kotlin("stdlib-jdk8"))
//    implementation(kotlin("reflect"))
    implementation("org.antlr:antlr4:$antlrVersion")
    implementation("org.jetbrains.kotlinx:kotlinx-collections-immutable:0.3.5")
    implementation("com.google.code.findbugs:jsr305:3.0.2")
    implementation("javax.inject:javax.inject:1")

    testImplementation("org.testng:testng:6.8")
    testImplementation("org.assertj:assertj-core:3.16.1")

    // https://mvnrepository.com/artifact/com.thoughtworks.xstream/xstream
    testImplementation(group = "com.thoughtworks.xstream", name = "xstream", version = "1.4.13")


    compileOnly("org.antlr:antlr4:$antlrVersion:complete")
    // I manually added this to the repo, since I couldn't get gradle to pull the antl4-complete tool jar.
}

//configure<JavaPluginConvention> {
//    sourceCompatibility = JavaVersion.VERSION_1_8
//}
java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(11))
    }
}

sourceSets {
    main {
        java {
            srcDir("src/gen/java")
        }
    }
}

tasks {
    compileKotlin {
        dependsOn("generateLexer", "generateParser")
        kotlinOptions.jvmTarget = "11"
    }
    compileTestKotlin {
        dependsOn("generateLexer", "generateParser")
        kotlinOptions.jvmTarget = "11"
    }

    // todo: use caching for up-to-date checks
    // up-to-date checks should be reasonably implementable
    // by hashing the grammarFile and the arguments given to the function
    // (remember, the cache should be invalidated if you change the packageName argument)
    // and save that hash to the output directory.
    register("generateLexer") {
        doLast {
            antlr("BabelLexer.g4")
        }
    }
    register("generateParser") {
        doLast {
            antlr("BabelParser.g4")
        }
    }

    test {
        useTestNG()
    }

    java {
        withSourcesJar()
        withJavadocJar()
    }
}

private val antlrToolJarRegex = Regex("antlr\\d?-.*-complete\\.jar")

fun Project.antlr(
        grammarFile: String,
        packageName: String = "$group.$name",
        outputDir: String = "src/gen/java",
        inputDir: String = "src/main/antlr"
) {
    val pkgPath = packageName.replace(".", "/")

//    val compileOnlyDepFiles = configurations.compileOnly.get().files
    val compileOnlyDepFiles = configurations.compileClasspath.get().files
    logger.debug("checking for antlr4 ($antlrToolJarRegex) configuartions.compileOnly.files: $compileOnlyDepFiles")
    val antlrJars = compileOnlyDepFiles.filter { it.name.matches(antlrToolJarRegex) }
    logger.debug("found antlr jars: ${antlrJars.joinToString("\n","\n")}")

    val antlrJar = antlrJars.firstOrNull()

    if(antlrJar == null || ! antlrJar.exists()) { throw RuntimeException("failed to find antlr compiler tool $antlrJar") }

    file(outputDir).mkdirs()

    val cmd = arrayOf(
            "java", "-jar", antlrJar.absolutePath,
            "-encoding", "UTF-8",
            "-o", "$outputDir/$pkgPath",
            "-package", packageName,
            "-lib", outputDir,
            "$inputDir/$grammarFile"
    )

    logger.debug("exec ~= ${cmd.joinToString(" ")}")
    val exec = Runtime.getRuntime().exec(cmd)
    val messages = `java.io`.BufferedReader(`java.io`.InputStreamReader(exec.errorStream)).use { err ->
        generateSequence { err.readLine() }.toList()
    }
    val result = exec.waitFor()

    if (result != 0) {
        throw RuntimeException(
                """antlr exec ~= ${cmd.joinToString(" ")}
                  |failed with code=${result} because:
                  |    ${messages.joinToString("\n")}
                """.trimMargin()
        )
    }

    return
}