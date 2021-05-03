package com.empowerops.babel

import org.antlr.v4.runtime.tree.ParseTree
import org.antlr.v4.runtime.tree.TerminalNode
import org.assertj.core.api.Assertions.assertThat
import org.testng.annotations.Test
import java.lang.StringBuilder
import java.lang.reflect.Modifier

class BabelRewriterFixture {

    @Test fun `when using sum with constant expr should be unrolled`(){
        //setup

        //act
        val parseTree = makeParseTree("sum(1, 3, i -> i);")

        //assert
        assertThat(parseTree.renderToSimpleString()).isEqualTo("""
                ${BabelParser.Scalar_evaluableContext::class.simpleName} {
                  StatementBlockContext availability=Runtime {
                    ReturnStatementContext {
                      ScalarExprContext availability=Runtime {
                        ScalarExprContext availability=Runtime {
                          ScalarExprContext availability=Runtime {
                            LambdaExprContext {
                              NameContext closedValue=1.0 { 'i' '[=1.0]' }
                              '->'
                              StatementBlockContext availability=Runtime {
                                ReturnStatementContext {
                                  ScalarExprContext availability=Runtime {
                                    VariableContext { 'i' }
                                  }
                                }
                              }
                            }
                          }
                          PlusContext { '+' }
                          ScalarExprContext availability=Runtime {
                            LambdaExprContext {
                              NameContext closedValue=2.0 { 'i' '[=2.0]' }
                              '->'
                              StatementBlockContext availability=Runtime {
                                ReturnStatementContext {
                                  ScalarExprContext availability=Runtime {
                                    VariableContext { 'i' }
                                  }
                                }
                              }
                            }
                          }
                        }
                        PlusContext { '+' }
                        ScalarExprContext availability=Runtime {
                          LambdaExprContext {
                            NameContext closedValue=3.0 { 'i' '[=3.0]' }
                            '->'
                            StatementBlockContext availability=Runtime {
                              ReturnStatementContext {
                                ScalarExprContext availability=Runtime {
                                  VariableContext { 'i' }
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                    ';'
                  }
                  '<EOF>'
                }
                
                """.trimIndent()
        )
    }

    private fun makeParseTree(program: String): ParseTree {

        var result: ParseTree? = null

        // simple hack to get the parse tree returned from the compile() function:
        // add a custom listener that records the root.

        BabelCompiler.compile(program, object: BabelParserBaseListener() {
            override fun exitScalar_evaluable(ctx: BabelParser.Scalar_evaluableContext?) {
                result = ctx!!
            }
        })

        return result!!
    }
}

