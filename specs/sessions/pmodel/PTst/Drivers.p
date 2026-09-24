/*****************************************************************************
 * Drivers.p — client machines that exercise the Session like the worker agent.
 *
 * Each driver issues lifecycle commands, waits for the reply (eActionDone /
 * eCmdRejected / eCleanupDone) before issuing the next — mirroring the real
 * "one action at a time" usage — and finally cleans up.
 *****************************************************************************/

/* A driver that walks a typical worker-agent pattern:
 *   enter job env → enter step env → run tasks → exit envs (LIFO) → cleanup.
 * With nondeterminism it may also cancel, hit failures, and try illegal
 * commands (which must be rejected, not crash).
 */
machine WorkerAgentDriver {
    var session: machine;
    var crossUser: bool;
    var tasksLeft: int;
    var tasksPlanned: bool;

    start state Init {
        entry (cu: bool) {
            crossUser = cu;
            session = new Session((crossUser = crossUser, hasCancelToken = false));
            goto EnterJobEnv;
        }
    }

    state EnterJobEnv {
        entry { send session, eEnterEnv, (client = this, envId = 1); }
        on eActionDone goto EnterStepEnv;
        on eCmdRejected goto CleanupNow; // env failed to enter → tear down
    }

    state EnterStepEnv {
        // The driver can't see Session internals; if the step-env enter is
        // rejected (e.g. the session went brittle), it handles eCmdRejected and
        // proceeds to teardown. No need to predict ending-only here.
        entry { send session, eEnterEnv, (client = this, envId = 2); }
        on eActionDone goto RunTasks;
        on eCmdRejected goto ExitStepEnv;
    }

    state RunTasks {
        entry {
            // Plan a bounded number of tasks ONCE, so the state space is finite.
            if (!tasksPlanned) { tasksLeft = choose(3); tasksPlanned = true; }
            if (tasksLeft > 0) {
                tasksLeft = tasksLeft - 1;
                send session, eRunTask, this;
            } else {
                goto ExitStepEnv;
            }
        }
        on eActionDone goto RunTasks;
        on eCmdRejected goto ExitStepEnv;
    }

    state ExitStepEnv {
        entry {
            // Exit env 2 if it's the top; otherwise skip to job-env exit.
            send session, eExitEnv, (client = this, envId = 2, keepRunning = false);
        }
        on eActionDone goto ExitJobEnv;
        on eCmdRejected goto ExitJobEnv;   // env 2 wasn't on top; try job env
    }

    state ExitJobEnv {
        entry { send session, eExitEnv, (client = this, envId = 1, keepRunning = false); }
        on eActionDone goto CleanupNow;
        on eCmdRejected goto CleanupNow;
    }

    state CleanupNow {
        entry { send session, eCleanup, this; }
        on eCleanupDone goto Done;
    }

    state Done { }
}

/* A driver that cancels the very first action, then must only be allowed to
 * exit environments and clean up (brittle-session contract). */
machine CancelDriver {
    var session: machine;
    start state Init {
        entry (cu: bool) {
            session = new Session((crossUser = cu, hasCancelToken = false));
            send session, eEnterEnv, (client = this, envId = 1);
            goto AwaitEnter;
        }
    }
    state AwaitEnter {
        // Fire a cancel as soon as we can; the Session is in Running. The
        // cancel may lose the race to a clean EOF, in which case onEnter
        // finishes Success and the session is NOT brittle — that's legal, so
        // the assertion below is guarded on the reported terminal state.
        entry { send session, eCancelAction, false; }
        on eActionDone do (st: tActionState) {
            if (st == ACT_SUCCESS) { goto ExitEnv; }  // cancel lost the race
            else { goto TryIllegalThenCleanup; }      // brittle now
        }
        on eCmdRejected goto Cleanup;
    }
    state TryIllegalThenCleanup {
        // The onEnter did NOT succeed, so the session is brittle: run_task must
        // be rejected.
        entry { send session, eRunTask, this; }
        on eCmdRejected goto ExitEnv;   // expected
        on eActionDone do (st: tActionState) {
            assert false, "run_task accepted after a failed/canceled action (brittle-session violated)";
        }
    }
    state ExitEnv {
        entry { send session, eExitEnv, (client = this, envId = 1, keepRunning = false); }
        on eActionDone goto Cleanup;
        on eCmdRejected goto Cleanup;
    }
    state Cleanup {
        entry { send session, eCleanup, this; }
        on eCleanupDone goto Done;
    }
    state Done { }
}

/* A driver that cancels via the EXTERNAL SessionConfig.cancel_token (a bare
 * token cancel — NOT cancel_action), mirroring a caller cancelling a whole
 * session from outside its async context. A token-only cancel must still take
 * effect on both the same-user and cross-user paths; CancelDeliverySpec asserts
 * this. (With CROSS_USER_HONORS_TOKEN_CANCEL flipped off, the cross-user run is
 * where that assertion bites — see Session.p.) Runs both crossUser values via
 * the test setup. */
machine ExternalCancelDriver {
    var session: machine;
    start state Init {
        entry (cu: bool) {
            session = new Session((crossUser = cu, hasCancelToken = true));
            send session, eEnterEnv, (client = this, envId = 1);
            goto AwaitEnter;
        }
    }
    state AwaitEnter {
        // Fire the EXTERNAL token cancel while the onEnter action is Running.
        entry { send session, eSessionCancelToken; }
        on eActionDone do (st: tActionState) { goto ExitEnv; }
        on eCmdRejected goto Cleanup;
    }
    state ExitEnv {
        entry { send session, eExitEnv, (client = this, envId = 1, keepRunning = false); }
        on eActionDone goto Cleanup;
        on eCmdRejected goto Cleanup;
    }
    state Cleanup {
        entry { send session, eCleanup, this; }
        on eCleanupDone goto Done;
    }
    state Done { }
}

/* A driver focused on the cross-user helper token security invariant:
 * a bad-token cancel must NOT stop a running action. We can't inject a bad
 * token through the Session (it always sends the right one), so this driver
 * talks to a helper directly. */
machine HelperTokenDriver {
    var helper: machine;
    var good: tToken;

    start state Init {
        entry {
            good = 7;
            helper = new CrossUserHelper((session = this, token = good));
            send helper, eHelperRun, (session = this, actionId = 100, token = good);
            goto Running;
        }
    }
    state Running {
        // Immediately try a cancel with the WRONG token.
        entry { send helper, eHelperCancel, (actionId = 100, method = CM_TERMINATE, token = good + 1); }
        on eHelperInvalidToken do { /* expected: run continues */ }
        ignore eHelperPid, eHelperOut, eHelperExited;
        on eProcessExited do (m: (actionId: tActionId, st: tActionState, code: tExitCode)) {
            // The action must have ended on its own terms (Success/Failed),
            // NOT Canceled — the bad-token cancel had no effect.
            assert m.st != ACT_CANCELED,
                "bad-token cancel canceled a running action (helper security violated)";
            send helper, eHelperShutdown, (session = this, token = good);
            goto Done;
        }
    }
    state Done {
        ignore eHelperPid, eHelperOut, eHelperExited, eHelperInvalidToken, eProcessExited;
    }
}
