package com.empowerops.babel

class Something {
    fun foo(){}
    fun bar(){}
}

val fooOrBar: Something.() -> Unit = if(true) {{ foo() }} else {{ bar() }} //compiles without issue

//val fooOrBar2: Something.() -> Unit = if(true) {{ foo() }} else if(true) {{ bar() }} else TODO() //fails: