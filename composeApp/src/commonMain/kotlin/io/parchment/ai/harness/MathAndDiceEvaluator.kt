package io.parchment.ai.harness

import kotlin.math.pow
import kotlin.random.Random

/**
 * Pure-Kotlin deterministic math evaluator (`calc_eval`) and unbiased dice roller (`dice_roll`).
 */
internal object MathAndDiceEvaluator {

    private val DICE_EXPR_REGEX = Regex("""^(\d*)d(\d+)([+-]\d+)?$""")

    fun evaluateDice(expression: String, reason: String): String {
        val cleaned = expression.trim().lowercase().replace(" ", "")
        val match = DICE_EXPR_REGEX.matchEntire(cleaned)
            ?: error("Invalid dice notation '$expression'. Expected format like '1d20' or '2d6+3'.")
        val count = (match.groupValues[1].toIntOrNull() ?: 1).coerceIn(1, 100)
        val sides = (match.groupValues[2].toIntOrNull() ?: 20).coerceIn(2, 10000)
        val modifier = match.groupValues[3].toIntOrNull() ?: 0
        val rolls = (1..count).map { Random.nextInt(1, sides + 1) }
        val sum = rolls.sum() + modifier
        val modStr = when {
            modifier > 0 -> " + $modifier"
            modifier < 0 -> " - ${-modifier}"
            else -> ""
        }
        val label = if (reason.isNotBlank()) " ($reason)" else ""
        return "Rolled $expression$label: [${rolls.joinToString(", ")}]$modStr = $sum"
    }

    fun evaluateMath(expression: String): String {
        val src = expression.trim()
        require(src.isNotEmpty()) { "Expression must not be empty." }
        val parser = MathExprParser(src)
        val value = parser.parseAll()
        val formatted = if (value.isFinite() && value == kotlin.math.floor(value) && kotlin.math.abs(value) < 9e15) {
            value.toLong().toString()
        } else {
            value.toString()
        }
        return "$src = $formatted"
    }

    private class MathExprParser(private val src: String) {
        private var pos = 0

        fun parseAll(): Double {
            val v = parseExpr()
            skipWs()
            require(pos == src.length) { "Unexpected trailing text '${src.substring(pos)}' in expression." }
            return v
        }

        private fun skipWs() {
            while (pos < src.length && (src[pos].isWhitespace() || src[pos] == '_')) pos++
        }

        private fun eat(ch: Char): Boolean {
            skipWs()
            if (pos < src.length && src[pos] == ch) {
                pos++
                return true
            }
            return false
        }

        private fun parseExpr(): Double {
            var x = parseTerm()
            while (true) {
                skipWs()
                when {
                    eat('+') -> x += parseTerm()
                    eat('-') -> x -= parseTerm()
                    else -> return x
                }
            }
        }

        private fun parseTerm(): Double {
            var x = parsePower()
            while (true) {
                skipWs()
                when {
                    eat('*') -> x *= parsePower()
                    eat('/') -> {
                        val d = parsePower()
                        require(d != 0.0) { "Division by zero." }
                        x /= d
                    }
                    eat('%') -> {
                        val d = parsePower()
                        require(d != 0.0) { "Modulo by zero." }
                        x %= d
                    }
                    else -> return x
                }
            }
        }

        private fun parsePower(): Double {
            val base = parseUnary()
            skipWs()
            return if (eat('^')) {
                base.pow(parsePower())
            } else {
                base
            }
        }

        private fun parseUnary(): Double {
            skipWs()
            if (eat('+')) return parseUnary()
            if (eat('-')) return -parseUnary()
            return parsePrimary()
        }

        private fun parsePrimary(): Double {
            skipWs()
            if (eat('(')) {
                val v = parseExpr()
                require(eat(')')) { "Missing closing parenthesis ')'." }
                return v
            }
            val start = pos
            if (pos < src.length && (src[pos].isDigit() || src[pos] == '.')) {
                while (pos < src.length && (src[pos].isDigit() || src[pos] == '.' || src[pos] == '_')) pos++
                if (pos < src.length && (src[pos] == 'e' || src[pos] == 'E')) {
                    pos++
                    if (pos < src.length && (src[pos] == '+' || src[pos] == '-')) pos++
                    while (pos < src.length && src[pos].isDigit()) pos++
                }
                val numStr = src.substring(start, pos).replace("_", "")
                return numStr.toDoubleOrNull() ?: error("Invalid number '$numStr'.")
            }
            if (pos < src.length && src[pos].isLetter()) {
                while (pos < src.length && (src[pos].isLetterOrDigit() || src[pos] == '_')) pos++
                val ident = src.substring(start, pos).lowercase()
                skipWs()
                if (eat('(')) {
                    val args = mutableListOf<Double>()
                    if (!eat(')')) {
                        do {
                            args.add(parseExpr())
                        } while (eat(','))
                        require(eat(')')) { "Missing ')' after function '$ident' arguments." }
                    }
                    require(args.isNotEmpty()) { "Function '$ident' requires at least 1 argument." }
                    val a0 = args[0]
                    return when (ident) {
                        "sqrt" -> kotlin.math.sqrt(a0)
                        "cbrt" -> kotlin.math.cbrt(a0)
                        "abs" -> kotlin.math.abs(a0)
                        "round" -> kotlin.math.round(a0)
                        "floor" -> kotlin.math.floor(a0)
                        "ceil" -> kotlin.math.ceil(a0)
                        "sin" -> kotlin.math.sin(a0)
                        "cos" -> kotlin.math.cos(a0)
                        "tan" -> kotlin.math.tan(a0)
                        "asin" -> kotlin.math.asin(a0)
                        "acos" -> kotlin.math.acos(a0)
                        "atan" -> kotlin.math.atan(a0)
                        "ln" -> kotlin.math.ln(a0)
                        "log", "log10" -> kotlin.math.log10(a0)
                        "log2" -> kotlin.math.log2(a0)
                        "exp" -> kotlin.math.exp(a0)
                        "pow" -> {
                            require(args.size >= 2) { "pow(x, y) requires 2 arguments." }
                            a0.pow(args[1])
                        }
                        "min" -> args.minOrNull() ?: a0
                        "max" -> args.maxOrNull() ?: a0
                        else -> error("Unknown math function '$ident'.")
                    }
                }
                return when (ident) {
                    "pi" -> kotlin.math.PI
                    "e" -> kotlin.math.E
                    else -> error("Unknown constant '$ident'.")
                }
            }
            error("Unexpected character at position $pos in '$src'.")
        }
    }
}
