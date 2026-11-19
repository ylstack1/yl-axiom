package io.parchment.ai.harness

import io.parchment.ai.model.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import kotlin.test.*

class ToolApprovalBrokerTest {
    private val policy = ToolAccessScope(
        AgentPermissionPolicy(
            workspacePath = "unused",
            workspaceMode = WorkspaceMode.READ_WRITE,
            approvalMode = ToolApprovalMode.ASK_EVERY_TOOL
        )
    )
    private val arguments = """{"path":"notes.md"}"""

    private val quiet = ToolAccessScope(policy.policy.copy(approvalMode = ToolApprovalMode.NEVER))

    @Test fun aQuestionWaitsForItsAnswerAndAnEveryToolPolicyDoesNotGateIt() = runBlocking {
        assertFalse(AgentPermissionGate.effect(policy, "ask_user", "{}").requiresApproval)
        val broker = ToolApprovalBroker("run", "chat", { quiet })
        val waiting = async { broker.askUser("call", "Which one?", listOf("A", "B")) }
        val question = withTimeout(2000) { broker.questions.first { it.isNotEmpty() }.single() }
        assertEquals(listOf("A", "B"), question.options)
        assertFalse(broker.answerQuestion("not-this-one", "A"))
        assertFalse(waiting.isCompleted)
        assertTrue(broker.answerQuestion(question.requestId, "B"))
        assertEquals("B", waiting.await())
        assertTrue(withTimeout(2000) { broker.questions.first { it.isEmpty() } }.isEmpty())
    }

    @Test fun stoppingOrChangingPermissionsWithdrawsAQuestion() = runBlocking {
        var current = quiet
        val broker = ToolApprovalBroker("run", "chat", { current })
        val changed = async { broker.askUser("call", "Still there?", emptyList()) }
        val first = withTimeout(2000) { broker.questions.first { it.isNotEmpty() }.single() }
        current = current.copy(policy = current.policy.copy(workspaceMode = WorkspaceMode.OFF))
        broker.refreshPolicy()
        assertNull(changed.await())
        assertFalse(broker.answerQuestion(first.requestId, "late"))
        val ended = async { broker.askUser("call", "Again?", emptyList()) }
        withTimeout(2000) { broker.questions.first { it.isNotEmpty() } }
        broker.close()
        assertNull(ended.await())
        assertNull(broker.askUser("call", "After the end?", emptyList()))
    }

    @Test fun oncePermitMatchesExactActionAndIsConsumed() = runBlocking {
        val broker = ToolApprovalBroker("run", "chat", { policy })
        val waiting = async { broker.authorize("call", "fs_read", arguments) }
        val request = withTimeout(2000) { broker.requests.first { it.isNotEmpty() }.single() }
        assertEquals("chat", request.chatId)
        assertEquals(arguments, request.argumentsJson)
        assertTrue(broker.respond(request.requestId, ToolApprovalDecision.ALLOW_ONCE))
        val permit = (waiting.await() as ToolAuthorization.Allowed).permit
        assertNotNull(permit.claim(policy, "fs_read", "{}"))
        assertNull(permit.claim(policy, "fs_read", arguments))
        assertNotNull(permit.claim(policy, "fs_read", arguments))
        assertTrue(broker.requests.value.isEmpty())
    }

    @Test fun runGrantIsExactAndRevokedEvenAfterPolicyChangesBack() = runBlocking {
        val broker = ToolApprovalBroker("run", "chat", { policy })
        val first = async { broker.authorize("first", "fs_read", arguments) }
        val request = withTimeout(2000) { broker.requests.first { it.isNotEmpty() }.single() }
        broker.respond(request.requestId, ToolApprovalDecision.ALLOW_EXACT_ACTION_FOR_RUN)
        val oldPermit = (first.await() as ToolAuthorization.Allowed).permit
        assertTrue(broker.authorize("second", "fs_read", arguments) is ToolAuthorization.Allowed)
        broker.invalidate()
        broker.refreshPolicy()
        assertNotNull(oldPermit.claim(policy, "fs_read", arguments))
        val next = async { broker.authorize("third", "fs_read", arguments) }
        val renewed = withTimeout(2000) { broker.requests.first { it.isNotEmpty() }.single() }
        broker.respond(renewed.requestId, ToolApprovalDecision.DENY)
        assertTrue(next.await() is ToolAuthorization.Denied)
        assertTrue(broker.authorize("fourth", "fs_read", arguments) is ToolAuthorization.Denied)
        assertTrue(broker.requests.value.isEmpty())
    }

    @Test fun policyAndFolderRevocationInvalidatePendingRequests() = runBlocking {
        var current = policy
        val broker = ToolApprovalBroker("run", "chat", { current })
        val waiting = async { broker.authorize("call", "fs_read", arguments) }
        val request = withTimeout(2000) { broker.requests.first { it.isNotEmpty() }.single() }
        current = policy.copy(policy = policy.policy.copy(workspacePath = "different"))
        assertFalse(broker.respond(request.requestId, ToolApprovalDecision.ALLOW_ONCE))
        assertTrue(waiting.await() is ToolAuthorization.Denied)
        assertTrue(broker.requests.value.isEmpty())
    }

    @Test fun cancellationAndStopDoNotAffectAnotherChat() = runBlocking {
        var stopped = false
        val firstBroker = ToolApprovalBroker("first", "first-chat", { policy }, { stopped = true })
        val secondBroker = ToolApprovalBroker("second", "second-chat", { policy })
        val first = async { firstBroker.authorize("first-call", "fs_read", arguments) }
        val second = async { secondBroker.authorize("second-call", "fs_read", arguments) }
        val firstRequest = withTimeout(2000) { firstBroker.requests.first { it.isNotEmpty() }.single() }
        val secondRequest = withTimeout(2000) { secondBroker.requests.first { it.isNotEmpty() }.single() }
        firstBroker.respond(firstRequest.requestId, ToolApprovalDecision.STOP)
        first.join()
        assertTrue(first.isCancelled)
        assertTrue(stopped)
        assertTrue(firstBroker.requests.value.isEmpty())
        assertFalse(second.isCompleted)
        assertTrue(secondBroker.respond(secondRequest.requestId, ToolApprovalDecision.ALLOW_ONCE))
        assertTrue(second.await() is ToolAuthorization.Allowed)
        assertTrue(firstBroker.authorize("late", "fs_read", arguments) is ToolAuthorization.Denied)
        val cancelling = async { secondBroker.authorize("cancelled", "fs_read", "{}") }
        withTimeout(2000) { secondBroker.requests.first { it.isNotEmpty() } }
        cancelling.cancelAndJoin()
        assertTrue(secondBroker.requests.value.isEmpty())
    }

    @Test fun processPendingRegistryRoutesPerActionApprovalsAndClearsCleanly() = runBlocking {
        val computerScope = ToolAccessScope(AgentPermissionPolicy(approvalMode = ToolApprovalMode.NEVER, computerAccess = true))
        val computerBroker = ToolApprovalBroker("comp-run", "comp-chat", { computerScope })
        val fileBroker = ToolApprovalBroker("file-run", "file-chat", { policy })
        val click = """{"action":"click","x":82,"y":276}"""
        try {
            val compWait = async { computerBroker.authorize("c1", ComputerUse.TOOL_NAME, click) }
            val fileWait = async { fileBroker.authorize("f1", "fs_read", arguments) }
            val compReq = withTimeout(2000) { computerBroker.requests.first { it.isNotEmpty() }.single() }
            val fileReq = withTimeout(2000) { fileBroker.requests.first { it.isNotEmpty() }.single() }
            assertTrue(ToolApprovalBroker.pendingRequests.value.map { it.requestId }.containsAll(listOf(compReq.requestId, fileReq.requestId)))

            assertTrue(ToolApprovalBroker.respondPending(compReq.requestId, ToolApprovalDecision.ALLOW_EXACT_ACTION_FOR_RUN))
            assertIs<ToolAuthorization.Allowed>(compWait.await())
            assertFalse(ToolApprovalBroker.pendingRequests.value.any { it.requestId == compReq.requestId })
            assertTrue(ToolApprovalBroker.pendingRequests.value.any { it.requestId == fileReq.requestId })

            // computer_use must still demand fresh explicit approval on the very next call.
            val compAgain = async { computerBroker.authorize("c2", ComputerUse.TOOL_NAME, click) }
            val compReq2 = withTimeout(2000) { ToolApprovalBroker.pendingRequests.first { list -> list.any { it.callId == "c2" } }.first { it.callId == "c2" } }
            assertTrue(ToolApprovalBroker.respondPending(compReq2.requestId, ToolApprovalDecision.DENY))
            assertIs<ToolAuthorization.Denied>(compAgain.await())

            assertTrue(ToolApprovalBroker.respondPending(fileReq.requestId, ToolApprovalDecision.ALLOW_ONCE))
            assertIs<ToolAuthorization.Allowed>(fileWait.await())
        } finally {
            computerBroker.close()
            fileBroker.close()
        }
        assertTrue(ToolApprovalBroker.pendingRequests.value.none { it.runId in setOf("comp-run", "file-run") })
    }
}
