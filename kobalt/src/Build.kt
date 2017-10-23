import com.beust.kobalt.*
import com.beust.kobalt.api.Project
import com.beust.kobalt.api.annotation.Task
import com.beust.kobalt.misc.error
import com.beust.kobalt.plugin.packaging.*
import com.beust.kobalt.plugin.publish.bintray
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader

val bs = buildScript {
    repos("http://dl.bintray.com/kotlin/kotlinx")
}
val p = project {
    name = "babel"
    group = "com.empowerops"
    artifactId = name
    version = "0.8"

    dependencies {
        compile("org.antlr:antlr4:4.7")
        compile("org.jetbrains.kotlinx:kotlinx-collections-immutable:0.1")
        compile("javax.inject:javax.inject:1")
    }

    dependenciesTest {
        compile("org.antlr:antlr4:jar:sources:4.7")
        compile("org.jetbrains.kotlinx:kotlinx-collections-immutable:jar:sources:0.1")

        compile("org.testng:testng:6.11")
        compile("org.assertj:assertj-core:3.8.0")
    }

    sourceDirectories {
        path("src/gen/java")
    }

    test {
        include("**/*Fixture.class")
    }

    bintray {
        publish = true
    }

    assemble {
        mavenJars {  }
    }
}

@Task(name = "cleanGenCode", reverseDependsOn = arrayOf("clean"))
fun cleanGeneratedCode(project: Project): TaskResult {
    File("src/gen/java").deleteRecursively()
    return TaskResult()
}

@Task(name = "antlr", reverseDependsOn = arrayOf("compile"), runAfter = arrayOf("clean"))
fun runAntlrTask(project: Project) : TaskResult {

    val pkg = project.run { "$group.$name" }

    antlr(pkg, "BabelLexer.g4")
    antlr(pkg, "BabelParser.g4")

    return TaskResult()
}

private fun antlr(
        packageName: String,
        grammarFile: String,
        outputDir: String = "src/gen/java",
        inputDir: String = "src/main/antlr"
) {
    val pkgPath = packageName.replace(".", "/")

    File(outputDir).mkdirs()

    val cmd = arrayOf(
            //            "java", "-version"
            "java", "-jar", "antlr-4.7-complete.jar",
            "-encoding", "UTF-8",
            "-o", "$outputDir/$pkgPath",
            "-package", packageName,
            "-lib", outputDir,
            "$inputDir/$grammarFile"
    )
    val exec = Runtime.getRuntime().exec(cmd)
    val messages = BufferedReader(InputStreamReader(exec.errorStream)).use { err ->
        generateSequence { err.readLine() }.toList()
    }
    val result = exec.waitFor()

    if (result != 0) {
        throw RuntimeException(
                """failed to run antlr
                  |    ${cmd.joinToString(" ")}
                  |because:
                  |    ${messages.joinToString("")}
                """.trimMargin()
        )
    }
}

