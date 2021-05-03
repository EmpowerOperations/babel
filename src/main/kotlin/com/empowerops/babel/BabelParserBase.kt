package com.empowerops.babel

import org.antlr.v4.runtime.Parser
import org.antlr.v4.runtime.TokenStream

abstract class BabelParserBase(input: TokenStream): Parser(input) {

    enum class Availability { Static, Runtime }
}