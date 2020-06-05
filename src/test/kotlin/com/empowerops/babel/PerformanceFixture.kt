package com.empowerops.babel

import org.testng.annotations.Test
import java.util.*
import kotlin.system.measureTimeMillis

class PerformanceFixture {

    val compiler = BabelCompiler()

    @Test fun `when running simple expression 50k times`(){

        //setup
        val runtime = compiler.compile("x1 + x2 > 20 - x3^2") as BabelExpression
        val random = Random()
        val warmupCount = 50
        val evalCount = 100_000
        val inputGrid = (0 until evalCount + warmupCount).map {
            DoubleArray(3) { random.nextDouble() * 20.0 }
        }
        //warmup
        for(index in 0 until warmupCount){
            runtime.evaluate(mapOf("x1" to 1.0, "x2" to 2.0, "x3" to 3.0))
        }

        //act
        val time = measureTimeMillis {
            for(index in warmupCount until (evalCount + warmupCount)){
                val row = inputGrid[index]
                val vector = java.util.Map.of("x1", row[0], "x2", row[1], "x3", row[2])
                runtime.evaluate(vector)
            }
        }

        //assert
        println("took ${time}ms for $evalCount evaluations")
    }
}