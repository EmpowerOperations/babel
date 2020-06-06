package com.empowerops.babel

import org.testng.annotations.Test
import java.util.*
import kotlin.system.measureTimeMillis

class PerformanceFixture {

    val compiler = BabelCompiler()

    @Test fun `when running simple expression 500k times`(){
        benchmark("x1 + x2 > 20 - x3^2", listOf("x1", "x2", "x3"), listOf(0.0 .. 20.0, 0.0 .. 20.0, 0.0 .. 20.0), 50, 500_000)
    }

    @Test fun `when running 200var prod 10k times`(){
        benchmark("sum(1, 200, i -> var[i]^2 - 3.0)", (1..200).map { "x$it" }, (1..200).map { 0.0 .. 10.0 }, 50, 10_000)
    }

    private fun benchmark(expr: String, vars: List<String>, bounds: List<ClosedRange<Double>>, warmupCount: Int, evalCount: Int) {

        require(vars.size == bounds.size)

        //setup
        val runtime = compiler.compile(expr) as BabelExpression
        val random = Random()
        val inputGrid = (0 until evalCount + warmupCount).map {
            DoubleArray(vars.size) { random.nextDouble() * bounds[it].span + bounds[it].start }
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
                if(index % 1000 == 0) print(".")
                val row = inputGrid[index]
                val vector = vars.withIndex().associate { (index, sym) -> sym to row[index] }
                runtime.evaluate(vector)
            }
        }
        println("")

        //assert
        println("took ${time}ms for $evalCount evaluations")
    }

    private val ClosedRange<Double>.span: Double get() = Math.abs(endInclusive - start)
}