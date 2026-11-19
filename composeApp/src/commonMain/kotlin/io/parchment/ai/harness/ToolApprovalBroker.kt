package io.parchment.ai.harness

import io.parchment.ai.runtime.RunActivityContext
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext

data class ToolApprovalRequest(
    val requestId: String,
    val runId: String,
    val chatId: String,
    val callId: String,
    val toolName: String,
    val argumentsJson: String,
    val scope: ToolAccessScope,
    val policyRevision: Long
)

enum class ToolApprovalDecision { ALLOW_ONCE, ALLOW_EXACT_ACTION_FOR_RUN, DENY, STOP }

data class UserQuestion(val requestId: String, val chatId: String, val callId: String, val question: String, val options: List<String>)

/** This capability is deliberately not serializable. */
class ToolExecutionPermit internal constructor(
    internal val broker: ToolApprovalBroker,
    internal val scope: ToolAccessScope,
    internal val toolName: String,
    internal val argumentsJson: String,
    internal val policyRevision: Long
) {
    private val consumed = MutableStateFlow(false)
    fun isRevoked(): Boolean = broker.isPermitRevoked(this)
    internal fun claim(scope: ToolAccessScope, name: String, arguments: String): String? {
        if (scope != this.scope || name != toolName || arguments != argumentsJson) return "Approval does not match this exact action."
        broker.permitDenial(this)?.let { return it }
        if (!consumed.compareAndSet(false, true)) return "This one-use execution permit was already consumed."
        return null
    }
}

sealed interface ToolAuthorization {
    data class Allowed(val permit: ToolExecutionPermit) : ToolAuthorization
    data class Denied(val reason: String) : ToolAuthorization
}

/** One broker per logical run, shared by parallel calls and continuation/compaction rounds. */
class ToolApprovalBroker(
    val runId: String,
    val chatId: String,
    private val policySource: () -> ToolAccessScope?,
    private val onStop: () -> Unit = {}
) {
    private data class Action(val name: String, val arguments: String)
    private data class Pending(val request: ToolApprovalRequest, val answer: CompletableDeferred<ToolApprovalDecision>)
    private val mutex = Mutex()
    private val pending = mutableMapOf<String, Pending>()
    private val grants = mutableSetOf<Action>()
    private val denials = mutableSetOf<Action>()
    private val requestsState = MutableStateFlow<List<ToolApprovalRequest>>(emptyList())
    val requests: StateFlow<List<ToolApprovalRequest>> = requestsState.asStateFlow()
    private data class PendingQuestion(val question: UserQuestion, val answer: CompletableDeferred<String?>)
    private val pendingQuestions = mutableMapOf<String, PendingQuestion>()
    private val questionsState = MutableStateFlow<List<UserQuestion>>(emptyList())
    val questions: StateFlow<List<UserQuestion>> = questionsState.asStateFlow()
    /** Set by the run that owns this broker, because only it can start and pay for a helper. */
    var subagentRunner: (suspend (task: String, onProgress: suspend (String) -> Unit) -> Result<String>)? = null
    private val closed = MutableStateFlow(false)
    private val revision = MutableStateFlow(0L)
    private var observedScope: ToolAccessScope? = policySource()
    private var observedRevision = 0L
    private var sequence = 0L

    fun currentScope(): ToolAccessScope? = if (closed.value) null else policySource()
    /** Call synchronously from policy/path edits so even an Off→On change invalidates old grants. */
    fun invalidate() { revision.update { it + 1L } }

    private fun publishRequestsLocked() {
        val currentRequests = pending.values.map { it.request }
        requestsState.value = currentRequests
        updateActiveBroker(this, currentRequests)
    }

    private fun reconcileLocked(): ToolAccessScope? {
        val current = currentScope()
        if (current != observedScope || observedRevision != revision.value) {
            if (current != observedScope) invalidate()
            grants.clear()
            denials.clear()
            pending.values.forEach { it.answer.complete(ToolApprovalDecision.DENY) }
            pending.clear()
            publishRequestsLocked()
            clearQuestionsLocked()
            observedScope = current
            observedRevision = revision.value
        }
        return current
    }

    private fun clearQuestionsLocked() {
        pendingQuestions.values.forEach { it.answer.complete(null) }
        pendingQuestions.clear()
        questionsState.value = emptyList()
    }

    /** Pauses the run for a person's answer. Null means the run ended or its permissions changed first. */
    suspend fun askUser(callId: String, question: String, options: List<String>): String? {
        currentCoroutineContext().ensureActive()
        val waiting = mutex.withLock {
            reconcileLocked() ?: return null
            val entry = PendingQuestion(UserQuestion("$runId:${++sequence}", chatId, callId, question, options), CompletableDeferred())
            pendingQuestions[entry.question.requestId] = entry
            questionsState.value = pendingQuestions.values.map { it.question }
            entry
        }
        val activity = currentCoroutineContext()[RunActivityContext]
        activity?.activity?.setWaiting(activity.branch, true)
        try {
            return waiting.answer.await()
        } finally {
            activity?.activity?.setWaiting(activity.branch, false)
            withContext(NonCancellable) {
                mutex.withLock {
                    pendingQuestions.remove(waiting.question.requestId)
                    questionsState.value = pendingQuestions.values.map { it.question }
                }
            }
        }
    }

    suspend fun answerQuestion(requestId: String, answer: String): Boolean = mutex.withLock {
        reconcileLocked()
        pendingQuestions[requestId]?.answer?.complete(answer) ?: false
    }

    private fun stopLocked() {
        closed.value = true
        grants.clear()
        denials.clear()
        pending.values.forEach { it.answer.complete(ToolApprovalDecision.STOP) }
        pending.clear()
        publishRequestsLocked()
        clearQuestionsLocked()
    }

    suspend fun refreshPolicy() { mutex.withLock { reconcileLocked() } }

    suspend fun authorize(callId: String, name: String, argumentsJson: String): ToolAuthorization {
        currentCoroutineContext().ensureActive()
        val action = Action(name, argumentsJson)
        var approval: Pending? = null
        val immediate = mutex.withLock {
            val scope = reconcileLocked() ?: return@withLock ToolAuthorization.Denied("This run is no longer active.")
            AgentPermissionGate.denial(scope, name, argumentsJson)?.let { return@withLock ToolAuthorization.Denied(it) }
            if (action in denials) return@withLock ToolAuthorization.Denied("This action was denied for this run. Change its permissions to reconsider it.")
            if (!AgentPermissionGate.effect(scope, name, argumentsJson).requiresApproval || action in grants) {
                return@withLock ToolAuthorization.Allowed(ToolExecutionPermit(this, scope, name, argumentsJson, revision.value))
            }
            val request = ToolApprovalRequest("$runId:${++sequence}", runId, chatId, callId, name, argumentsJson, scope, revision.value)
            val waiting = Pending(request, CompletableDeferred())
            approval = waiting
            pending[request.requestId] = waiting
            publishRequestsLocked()
            null
        }
        if (immediate != null) return immediate
        val waiting = requireNotNull(approval)
        val activity = currentCoroutineContext()[RunActivityContext]
        activity?.activity?.setWaiting(activity.branch, true)
        try {
            val decision = waiting.answer.await()
            currentCoroutineContext().ensureActive()
            if (decision == ToolApprovalDecision.STOP) throw CancellationException("Stopped while awaiting tool approval.")
            return mutex.withLock {
                val scope = reconcileLocked()
                if (scope == null || scope != waiting.request.scope || revision.value != waiting.request.policyRevision) return@withLock ToolAuthorization.Denied("Permissions changed while approval was pending.")
                AgentPermissionGate.denial(scope, name, argumentsJson)?.let { return@withLock ToolAuthorization.Denied(it) }
                when (decision) {
                    ToolApprovalDecision.ALLOW_ONCE, ToolApprovalDecision.ALLOW_EXACT_ACTION_FOR_RUN -> {
                        if (decision == ToolApprovalDecision.ALLOW_EXACT_ACTION_FOR_RUN && name != ComputerUse.TOOL_NAME) grants.add(action)
                        ToolAuthorization.Allowed(ToolExecutionPermit(this, scope, name, argumentsJson, revision.value))
                    }
                    else -> {
                        denials.add(action)
                        ToolAuthorization.Denied("Action denied by the user.")
                    }
                }
            }
        } finally {
            activity?.activity?.setWaiting(activity.branch, false)
            withContext(NonCancellable) {
                mutex.withLock {
                    pending.remove(waiting.request.requestId)
                    publishRequestsLocked()
                }
            }
        }
    }

    suspend fun respond(requestId: String, decision: ToolApprovalDecision): Boolean {
        val accepted = mutex.withLock {
            reconcileLocked()
            val waiting = pending[requestId] ?: return@withLock false
            if (decision == ToolApprovalDecision.STOP) {
                stopLocked()
                true
            } else waiting.answer.complete(decision)
        }
        if (accepted && decision == ToolApprovalDecision.STOP) onStop()
        return accepted
    }

    suspend fun close() {
        mutex.withLock {
            closed.value = true
            reconcileLocked()
        }
    }

    internal fun permitDenial(permit: ToolExecutionPermit): String? {
        val scope = currentScope()
        if (scope == null || scope != permit.scope || revision.value != permit.policyRevision) return "Tool permission was revoked before execution."
        return AgentPermissionGate.denial(scope, permit.toolName, permit.argumentsJson)
    }

    internal fun isPermitRevoked(permit: ToolExecutionPermit): Boolean =
        closed.value || revision.value != permit.policyRevision || currentScope() != permit.scope

    companion object {
        private val activeLock = Any()
        private val activeEntries = LinkedHashMap<ToolApprovalBroker, List<ToolApprovalRequest>>()
        private val pendingRequestsState = MutableStateFlow<List<ToolApprovalRequest>>(emptyList())
        /** Pending requests across active runs in this process, for out-of-activity system surfaces. */
        val pendingRequests: StateFlow<List<ToolApprovalRequest>> = pendingRequestsState.asStateFlow()

        private fun updateActiveBroker(broker: ToolApprovalBroker, requests: List<ToolApprovalRequest>) {
            synchronized(activeLock) {
                if (requests.isEmpty()) activeEntries.remove(broker) else activeEntries[broker] = requests
                pendingRequestsState.value = activeEntries.values.flatten()
            }
        }

        suspend fun respondPending(requestId: String, decision: ToolApprovalDecision): Boolean {
            val target = synchronized(activeLock) {
                activeEntries.entries.firstOrNull { (_, list) -> list.any { it.requestId == requestId } }?.key
            } ?: return false
            return target.respond(requestId, decision)
        }
    }
}

/** One command of a compound shell line; [joinedBy] is the operator before it ("" for the first). */
data class ShellPreviewPart(val joinedBy: String, val command: String)

/**
 * A display-only breakdown of a shell command for the approval panel. It never changes what runs: the permit
 * still binds the exact, unmodified arguments ([ToolExecutionPermit]).
 */
data class ShellApprovalPreview(
    val parts: List<ShellPreviewPart>,
    val writesFiles: Boolean,
    val nestedCommands: Boolean,
    val runsInBackground: Boolean
) {
    /** Worth showing only when the line is more than one plain command. */
    val isCompound: Boolean get() = parts.size > 1 || writesFiles || nestedCommands || runsInBackground
}

/**
 * Splits [command] at top-level `&&`, `||`, `;`, `|` and line breaks, ignoring operators inside quotes or after
 * an escape (`\` for sh, the backtick for PowerShell), and flags redirections (`>`), nested commands (`$(`)
 * and a trailing background `&`. Display only, so a parse that is wrong for an exotic shell costs nothing.
 */
fun parseShellApprovalPreview(command: String): ShellApprovalPreview {
    val parts = ArrayList<ShellPreviewPart>()
    val current = StringBuilder()
    var joinedBy = ""
    var inSingle = false
    var inDouble = false
    var writes = false
    var nested = false
    var background = false
    fun flush(nextOperator: String) {
        val text = current.toString().trim()
        if (text.isNotEmpty()) parts += ShellPreviewPart(joinedBy, text)
        if (text.isNotEmpty()) joinedBy = nextOperator
        current.setLength(0)
    }
    var i = 0
    while (i < command.length) {
        val c = command[i]
        val next = command.getOrNull(i + 1)
        when {
            (c == '\\' || c == '`') && !inSingle && next != null -> {
                current.append(c).append(next); i += 2; continue
            }
            c == '\'' && !inDouble -> inSingle = !inSingle
            c == '"' && !inSingle -> inDouble = !inDouble
            inSingle -> Unit
            c == '$' && next == '(' -> nested = true
            inDouble -> Unit
            (c == '&' && next == '&') || (c == '|' && next == '|') -> {
                flush("$c$c"); i += 2; continue
            }
            c == ';' || c == '\n' -> { flush(";"); i++; continue }
            c == '|' -> { flush("|"); i++; continue }
            c == '>' -> if (next != '&') writes = true
            c == '&' && command.substring(i + 1).trimStart().let { it.isEmpty() || it[0] == ';' || it[0] == '\n' } &&
                command.getOrNull(i - 1) != '>' -> {
                background = true; flush(";"); i++; continue
            }
        }
        current.append(c)
        i++
    }
    flush("")
    return ShellApprovalPreview(parts, writes, nested, background)
}

/**
 * Run-scoped file revision tracker for the stale-read guard: remembers the disk fingerprint of every
 * workspace file read or written during the current run so edits/overwrites never clobber unread or
 * externally modified files.
 */
class WorkspaceFileRevisionTracker {
    private val lock = Any()
    private val seenStamps = HashMap<String, String>()

    private fun normalize(relativePath: String): String =
        relativePath.trim().replace('\\', '/').trimStart('/').lowercase()

    fun recordRead(relativePath: String, stamp: String?) {
        if (stamp == null) return
        val key = normalize(relativePath)
        if (key.isEmpty()) return
        synchronized(lock) { seenStamps[key] = stamp }
    }

    fun recordWrite(relativePath: String, stamp: String?) {
        if (stamp == null) return
        val key = normalize(relativePath)
        if (key.isEmpty()) return
        synchronized(lock) { seenStamps[key] = stamp }
    }

    fun recordDeleted(relativePath: String) {
        val key = normalize(relativePath)
        if (key.isEmpty()) return
        synchronized(lock) { seenStamps.remove(key) }
    }

    fun checkWriteAllowed(relativePath: String, currentDiskStamp: String?): String? {
        // Creating a brand-new file that does not yet exist on disk never requires a prior fs_read.
        if (currentDiskStamp == null) return null
        val key = normalize(relativePath)
        val seen = synchronized(lock) { seenStamps[key] }
        return when {
            seen == null ->
                "Stale-read guard: '$relativePath' already exists and has not been read in this run. Call fs_read on '$relativePath' before editing or overwriting it."
            seen != currentDiskStamp ->
                "Stale-read guard: '$relativePath' changed on disk since your last fs_read. Call fs_read on '$relativePath' again before applying edits."
            else -> null
        }
    }
}

class AgentToolRunContext(
    val broker: ToolApprovalBroker?,
    val fileRevisions: WorkspaceFileRevisionTracker = WorkspaceFileRevisionTracker()
) : AbstractCoroutineContextElement(Key) {
    private val mcpLock = Any()
    private val expandedMcp = LinkedHashSet<String>()

    fun expandMcpTools(names: Collection<String>) {
        if (names.isEmpty()) return
        synchronized(mcpLock) { expandedMcp.addAll(names) }
    }

    fun expandedMcpToolsSnapshot(): Set<String> = synchronized(mcpLock) { expandedMcp.toSet() }

    companion object Key : CoroutineContext.Key<AgentToolRunContext>
}
