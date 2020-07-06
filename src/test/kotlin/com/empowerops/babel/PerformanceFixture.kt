package com.empowerops.babel

import org.testng.annotations.Test
import java.util.*
import kotlin.system.measureTimeMillis

class PerformanceFixture {

    init {
        println("max memory is ${Runtime.getRuntime().maxMemory() / (1024*1024) / 1024.0} GB")
    }

    val compiler = BabelCompiler()

    // 781825ba3fbc07a2dc8f2d3d331cdf98b474f9f7 -- sealed class, int code get() = ...
    //      2283ms +/- 300ms on desktop
    // bd76e605a6549eb85e4a3c405060d30fe5c910a5 -- using sealed class with val code: Int switch
    //      2239ms +/- 100ms on desktop
    // 9da47ceb0739664665ae059675ad9946524dcf40 -- using unrolled instruction class, switch on int (not type)
    //      2990ms +/- 100ms on desktop
    // 0e6491c116f88451bff72af192e4311c146b885a -- using Instruction sealed class
    //      1988ms +/- 200ms on desktop
    // ba60d81e6416931b1b21952a6f17686b71a8f2d7 -- master (tree evaluation)
    //      2381ms +/- 100ms on desktop
    @Test(groups = ["performance"]) fun `when running simple expression millions of times`(){
        benchmark("x1 + x2 > 20 - x3^2", listOf("x1", "x2", "x3"), listOf(0.0 .. 20.0, 0.0 .. 20.0, 0.0 .. 20.0), 50, 5_000_000)
    }

    // 781825ba3fbc07a2dc8f2d3d331cdf98b474f9f7 -- sealed class, int code get() = ...
    //      1021ms +/- 100ms on desktop
    // bd76e605a6549eb85e4a3c405060d30fe5c910a5 -- using sealed class with val code: Int switch
    //      845ms +/- 100ms on desktop
    // 9da47ceb0739664665ae059675ad9946524dcf40 -- using unrolled instruction class, switch on int (not type)
    //      878ms +/- 100ms on desktop
    // 0e6491c116f88451bff72af192e4311c146b885a -- using Instruction sealed class
    //      853ms +/- 200ms on desktop
    // ba60d81e6416931b1b21952a6f17686b71a8f2d7 -- master (tree evaluation)
    //      2169ms +/- 300ms on desktop
    @Test(groups = ["performance"]) fun `when running 200var prod 10k times`(){
        benchmark("sum(1, 200, i -> var[i]^2 - 3.0)", (1..200).map { "x$it" }, (1..200).map { 0.0 .. 10.0 }, 50, 10_000)
    }

    private fun benchmark(expr: String, vars: List<String>, bounds: List<ClosedRange<Double>>, warmupCount: Int, evalCount: Int) {

        require(vars.size == bounds.size)
        println("running $evalCount iterations of $expr...")

        //setup
        val runtime = compiler.compile(expr) as BabelExpression
        val random = Random()
        var failedIteration: Int = -1
        val inputGrid = try {
            (0 until evalCount + warmupCount).map {
                try {
                    DoubleArray(vars.size) { random.nextDouble() * bounds[it].span + bounds[it].start }
                }
                catch(err: OutOfMemoryError){
                    failedIteration = it
                    throw err
                }
            }
        }
        catch(ex: OutOfMemoryError){
            System.gc()
            System.err.println("failed on grid allocation $failedIteration")
            throw ex
        }
        //warmup
        for (index in 0 until warmupCount) {
            val row = inputGrid[index]
            val vector = vars.withIndex().associate { (index, sym) -> sym to row[index] }
            runtime.evaluate(vector)
        }

        //act
        val time = measureTimeMillis {
            for (index in warmupCount until (evalCount + warmupCount)) {
                val row = inputGrid[index]
                val vector = vars.withIndex().associate { (index, sym) -> sym to row[index] }
                runtime.evaluate(vector)
            }
        }

        //assert
        println("took ${time}ms for $evalCount evaluations")
    }
}