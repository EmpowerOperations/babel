import com.beust.kobalt.*
import com.beust.kobalt.plugin.packaging.*
import com.beust.kobalt.plugin.publish.bintray


val bs = buildScript {
    repos("http://dl.bintray.com/kotlin/kotlinx")
}
val p = project {
    name = "babel"
    group = "com.empowerops"
    artifactId = name
    version = "0.7"

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
        path("src/gen/")
    }

    test {
        include("**/*Fixture")
    }

    bintray {
        publish = true
    }

    assemble {
        mavenJars {  }
    }
}
