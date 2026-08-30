package com.empowerops.babel

import org.testng.annotations.Test

/**
 * How fast does the JVM babel evaluate?
 *
 * The counterpart to the Rust `throughput_benchmarks.rs`, using the same
 * expressions and printing the same columns so the two can be read side by side.
 *
 * This exists rather than reusing [PerformanceFixture] because that one cannot
 * be believed. Three separate things sit inside its timed region:
 *
 *  - a `Map` is rebuilt every iteration, which for the 200-variable case means
 *    two hundred boxed doubles and a hash table per evaluation;
 *  - `print(".")` runs every thousandth iteration, putting synchronised console
 *    I/O in the measurement;
 *  - warm-up is fifty iterations, where tiered HotSpot wants on the order of ten
 *    thousand invocations before C2 compiles anything.
 *
 * So its number describes map allocation and console writes in a largely
 * interpreted tier. It is left in place as a record of what the old figure
 * meant; this is what replaces it.
 *
 * Not JMH. That would want a Gradle plugin and a dependency for a measurement
 * whose real flaws are all fixable by hand. What is given up, and is worth
 * knowing before quoting these numbers: no forked JVM, no blackhole against
 * JIT-level dead code elimination beyond the accumulator below, no calibration
 * of per-iteration harness overhead, and no defence against on-stack
 * replacement. Revisit if a number looks impossible.
 */
class ThroughputBenchmarks {

    private data class Case(val name: String, val source: String, val variables: Int)

    /** Warm-up per case. Generous on purpose: C2 needs the invocations. */
    private val warmupMillis = 3_000L

    /** Measurement window per repetition. */
    private val measureMillis = 400L

    /** Best of this many — scheduling noise only ever makes things slower. */
    private val repetitions = 3

    /** Distinct input rows to cycle through, so nothing can be hoisted. */
    private val rows = 256

    @Test
    fun `evaluation throughput`() {
        val cases = listOf(
            Case("trivial", "x1 + x2", 2),
            Case("small (jvm)", "x1 + x2 > 20 - x3^2", 3),
            Case("transcendental", "sin(x1) * cos(x2) + sqrt(abs(x3))", 3),
            Case(
                "deep arithmetic",
                "(((((x1 + x2) * x3 - x4) / (x1 + 1) + x2) * x3 - x4) / (x1 + 1) + x2) " +
                    "* x3 - x4 + ((x1 * x2) - (x3 / (x4 + 1)))^2",
                4,
            ),
            Case("200-var sum (jvm)", "sum(1, 200, i -> var[i]^2 - 3.0)", 200),
        )

        val results = cases.map { it to measure(it) }

        println()
        println(
            "babel evaluation throughput, points/ms " +
                "(JVM ${System.getProperty("java.version")}, ${System.getProperty("java.vm.name")})"
        )
        println("-".repeat(64))
        println("%-20s %5s %12s".format("expression", "vars", "map"))
        for ((case, rate) in results) {
            println("%-20s %5d %12.1f".format(case.name, case.variables, rate))
        }
        println("-".repeat(64))
        println("map = BabelExpression.evaluate(Map<String, Double>), given a pre-built map.")
        println("The map build is deliberately outside the timed region.")
        println()
    }

    private fun measure(case: Case): Double {
        val names = (1..case.variables).map { "x$it" }
        val expression = BabelCompiler.compile(case.source) as? BabelExpression
            ?: throw AssertionError("${case.name} did not compile")

        // Built once. A benchmark that allocates per iteration measures the
        // allocator, which is the mistake this file exists to correct.
        val random = java.util.Random(0x8E4C479A11L)
        val inputs = (0 until rows).map {
            names.associateWith { 0.1 + random.nextDouble() * 9.9 }
        }

        // `sink` is accumulated and printed so the JIT cannot decide the calls
        // are pointless. Crude next to a JMH blackhole, and enough here.
        var sink = 0.0
        var warmupCount = 0L

        val warmupEnd = System.nanoTime() + warmupMillis * 1_000_000
        while (System.nanoTime() < warmupEnd) {
            repeat(64) {
                sink += expression.evaluate(inputs[(warmupCount++ % rows).toInt()])
            }
        }

        var best = 0.0
        repeat(repetitions) {
            var count = 0L
            val start = System.nanoTime()
            val deadline = start + measureMillis * 1_000_000
            while (System.nanoTime() < deadline) {
                repeat(64) {
                    sink += expression.evaluate(inputs[(count++ % rows).toInt()])
                }
            }
            val elapsedSeconds = (System.nanoTime() - start) / 1_000_000_000.0
            best = maxOf(best, count / elapsedSeconds / 1000.0)
        }

        // Consuming the accumulator, which is the whole reason it exists.
        if (sink == Double.POSITIVE_INFINITY) println("unreachable: $sink")
        return best
    }
}
