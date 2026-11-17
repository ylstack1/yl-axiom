package io.parchment.ai.platform

internal data class FuzzyEditResult(
    val updatedLf: String,
    val addedLines: Int,
    val removedLines: Int,
    val usedFuzzyWhitespace: Boolean
)

internal object WorkspaceFuzzyEditEngine {
    private val LINE_NUMBER_PREFIX_REGEX = Regex("""^\s*(\d+):\s?(.*)$""")

    /**
     * Strips accidental `123: ` line-number prefixes from `old_text` / `new_text` when a model copies
     * lines directly from `fs_read` output (`File: ... \n 1: ... \n 2: ...`), unless the target file itself
     * actually contains those literal `123: ` prefixes.
     */
    fun stripAccidentalReadLinePrefixes(
        candidate: String,
        existingFileLf: String,
        requireStrippedInFile: Boolean = true
    ): String {
        val lines = candidate.lines()
        if (lines.size < 2) return candidate
        var matchedSeq = 0
        var prevNum = -1
        val strippedLines = mutableListOf<String>()
        for (line in lines) {
            if (line.isBlank()) {
                strippedLines.add(line)
                continue
            }
            val m = LINE_NUMBER_PREFIX_REGEX.matchEntire(line)
            if (m != null) {
                val num = m.groupValues[1].toIntOrNull() ?: -1
                if (prevNum == -1 || num == prevNum + 1) {
                    prevNum = num
                    matchedSeq++
                    strippedLines.add(m.groupValues[2])
                } else {
                    return candidate
                }
            } else {
                return candidate
            }
        }
        if (matchedSeq < 2) return candidate
        val stripped = strippedLines.joinToString("\n")
        if (existingFileLf.contains(candidate)) return candidate
        return if (!requireStrippedInFile || existingFileLf.contains(stripped)) {
            stripped
        } else {
            candidate
        }
    }

    /**
     * 2-Tier Exact + Whitespace/Indentation-Normalized Self-Healing Edit Engine:
     * 1. Tier 1: Exact substring match (with automatic stripping of accidental `fs_read` line numbers).
     * 2. Tier 2: Line-by-line trimmed/whitespace-normalized match when exact match fails due to minor
     *    indentation or trailing-space drift, keeping the relative indentation of the replacement.
     */
    fun applySingleEditWithFuzzySelfHealing(
        existingLf: String,
        rawOldText: String,
        rawNewText: String,
        replaceAll: Boolean,
        relativePath: String
    ): FuzzyEditResult {
        require(rawOldText.isNotEmpty()) { "old_text must not be empty." }
        val normOldRaw = rawOldText.replace("\r\n", "\n")
        val normNewRaw = rawNewText.replace("\r\n", "\n")
        val normOld = stripAccidentalReadLinePrefixes(normOldRaw, existingLf, requireStrippedInFile = false)
        val normNew = stripAccidentalReadLinePrefixes(normNewRaw, existingLf, requireStrippedInFile = false)

        // Tier 1: Exact Match
        if (existingLf.contains(normOld)) {
            val updated = if (replaceAll) {
                existingLf.replace(normOld, normNew)
            } else {
                val firstIdx = existingLf.indexOf(normOld)
                val secondIdx = existingLf.indexOf(normOld, startIndex = firstIdx + normOld.length)
                if (secondIdx != -1) {
                    val matchLines = mutableListOf<Int>()
                    var searchIdx = 0
                    while (true) {
                        val hit = existingLf.indexOf(normOld, startIndex = searchIdx)
                        if (hit == -1) break
                        val lineNum = existingLf.substring(0, hit).count { it == '\n' } + 1
                        matchLines.add(lineNum)
                        searchIdx = hit + normOld.length.coerceAtLeast(1)
                    }
                    error("old_text matched ${matchLines.size} locations in '$relativePath' (at lines ${matchLines.joinToString(", ")}). Include more surrounding context or set replace_all=true.")
                }
                existingLf.replaceFirst(normOld, normNew)
            }
            return FuzzyEditResult(
                updatedLf = updated,
                addedLines = normNew.lines().size,
                removedLines = normOld.lines().size,
                usedFuzzyWhitespace = false
            )
        }

        // Tier 2: Line-by-line Whitespace / Indentation Fuzzy Match
        val fileLines = existingLf.split('\n')
        val oldLines = normOld.split('\n')
        if (oldLines.isNotEmpty() && oldLines.size <= fileLines.size) {
            val oldTrimmed = oldLines.map { it.trim() }
            if (oldTrimmed.any { it.isNotEmpty() }) {
                val matchStartIndices = mutableListOf<Int>()
                val maxStart = fileLines.size - oldLines.size
                for (startIdx in 0..maxStart) {
                    var allMatch = true
                    for (j in oldLines.indices) {
                        if (fileLines[startIdx + j].trim() != oldTrimmed[j]) {
                            allMatch = false
                            break
                        }
                    }
                    if (allMatch) {
                        matchStartIndices.add(startIdx)
                    }
                }

                if (matchStartIndices.size == 1 || (replaceAll && matchStartIndices.isNotEmpty())) {
                    val newLinesRaw = normNew.split('\n')
                    val firstNonBlankOldIdx = oldLines.indexOfFirst { it.isNotBlank() }
                    val oldBaseIndent = if (firstNonBlankOldIdx >= 0) {
                        oldLines[firstNonBlankOldIdx].takeWhile { it == ' ' || it == '\t' }.length
                    } else 0

                    val resultLines = mutableListOf<String>()
                    var cursor = 0
                    for (matchStart in matchStartIndices) {
                        if (matchStart < cursor) continue
                        resultLines.addAll(fileLines.subList(cursor, matchStart))
                        val fileBaseIndentStr = if (firstNonBlankOldIdx >= 0) {
                            fileLines[matchStart + firstNonBlankOldIdx].takeWhile { it == ' ' || it == '\t' }
                        } else ""
                        val adjustedNewLines = newLinesRaw.map { nl ->
                            if (nl.isBlank()) ""
                            else {
                                val nlIndent = nl.takeWhile { it == ' ' || it == '\t' }.length
                                val relIndent = (nlIndent - oldBaseIndent).coerceAtLeast(0)
                                fileBaseIndentStr + " ".repeat(relIndent) + nl.trimStart()
                            }
                        }
                        resultLines.addAll(adjustedNewLines)
                        cursor = matchStart + oldLines.size
                    }
                    resultLines.addAll(fileLines.subList(cursor, fileLines.size))
                    return FuzzyEditResult(
                        updatedLf = resultLines.joinToString("\n"),
                        addedLines = newLinesRaw.size,
                        removedLines = oldLines.size,
                        usedFuzzyWhitespace = true
                    )
                } else if (matchStartIndices.size > 1) {
                    val lineNums = matchStartIndices.map { it + 1 }
                    error("old_text fuzzy-matched ${lineNums.size} locations in '$relativePath' (at lines ${lineNums.joinToString(", ")}). Include more surrounding lines or set replace_all=true.")
                }
            }
        }

        val hint = findClosestNearMissHint(fileLines, oldLines)
        error("old_text was not found in '$relativePath' (${fileLines.size} lines).${if (hint.isNotEmpty()) "\n$hint" else " Use fs_read to inspect the exact lines before editing."}")
    }

    private fun findClosestNearMissHint(fileLines: List<String>, oldLines: List<String>): String {
        if (fileLines.isEmpty() || oldLines.isEmpty()) return ""
        val needles = oldLines.map { it.trim() }.filter { it.length >= 4 }.take(12)
        if (needles.isEmpty()) return ""
        val needleSet = needles.toSet()
        val windowSize = oldLines.size.coerceIn(1, fileLines.size)
        val firstAnchor = needles.first()
        var bestStart = -1
        var bestScore = 0
        val fileTrimmed = fileLines.map { it.trim() }
        val maxStart = (fileLines.size - windowSize).coerceAtLeast(0)
        for (start in 0..maxStart) {
            var score = 0
            val endExclusive = (start + windowSize).coerceAtMost(fileTrimmed.size)
            if (fileTrimmed[start] == firstAnchor) score += 3
            else if (firstAnchor.length >= 8 && fileTrimmed[start].contains(firstAnchor, ignoreCase = true)) score += 2
            for (idx in start until endExclusive) {
                val line = fileTrimmed[idx]
                if (line.length >= 4 && line in needleSet) score += 2
            }
            if (score > bestScore) {
                bestScore = score
                bestStart = start
            }
        }
        val minRequiredScore = if (needles.size == 1) 2 else 3
        if (bestStart < 0 || bestScore < minRequiredScore) return ""
        val previewFrom = (bestStart - 1).coerceAtLeast(0)
        val previewToExclusive = (bestStart + windowSize.coerceAtMost(8) + 1).coerceAtMost(fileLines.size)
        return buildString {
            appendLine("Closest match at lines ${previewFrom + 1}..$previewToExclusive:")
            for (idx in previewFrom until previewToExclusive) {
                appendLine("${idx + 1}: ${fileLines[idx].take(200)}")
            }
        }.trimEnd()
    }
}
