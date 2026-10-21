/*****************************************************************************
 * SessionStateSpec.p — the SessionState transition + brittle-session invariants
 * (specs/sessions/session.md § Transitions, § Brittle Sessions).
 *
 * Asserts:
 *   - only documented transitions occur
 *   - Ended is terminal (no transition out)
 *   - at most one action Running at a time
 *   - after any Failed/Canceled/Timeout the session is ending-only: it may only
 *     reach Ready via S_READY_ENDING, never plain S_READY again
 *****************************************************************************/

spec SessionStateSpec observes eMonStateChanged, eMonActionStart, eMonActionEnd {
    var actionsRunning: int;   // must never exceed 1
    var brittle: bool;         // a terminal-failure action has occurred

    start state Watching {
        entry { actionsRunning = 0; brittle = false; }

        on eMonStateChanged do (t: (fromState: tSessionState, toState: tSessionState)) {
            assert validTransition(t.fromState, t.toState),
                format ("Illegal SessionState transition {0} -> {1}", t.fromState, t.toState);

            // Ended is terminal.
            assert t.fromState != S_ENDED,
                "SessionState left the terminal Ended state";

            // Once brittle, the session must not return to plain Ready. The
            // only allowed non-ending resting state after a failure is
            // S_READY_ENDING (reached via ReadyEnding) or S_ENDED.
            if (brittle) {
                assert t.toState != S_READY,
                    "Brittle session transitioned back to Ready (should be ReadyEnding)";
            }
        }

        on eMonActionStart do (m: (actionId: tActionId, kind: tActionKind)) {
            actionsRunning = actionsRunning + 1;
            assert actionsRunning <= 1,
                format ("More than one action running concurrently ({0})", actionsRunning);
        }

        on eMonActionEnd do (m: (actionId: tActionId, st: tActionState)) {
            actionsRunning = actionsRunning - 1;
            assert actionsRunning >= 0, "action ended without a matching start";
            if (m.st == ACT_FAILED || m.st == ACT_CANCELED || m.st == ACT_TIMEOUT) {
                brittle = true;
            }
        }
    }
}

/* The transition relation from specs/sessions/session.md § Transitions. */
fun validTransition(fromS: tSessionState, toS: tSessionState): bool {
    if (fromS == toS) { return true; } // announce fires even for no-op re-sets
    // Ready → Running | Ended
    if (fromS == S_READY)        { return toS == S_RUNNING || toS == S_ENDED; }
    // Running → Ready | ReadyEnding | Canceling
    if (fromS == S_RUNNING)      { return toS == S_READY || toS == S_READY_ENDING || toS == S_CANCELING; }
    // Canceling → Ready | ReadyEnding
    if (fromS == S_CANCELING)    { return toS == S_READY || toS == S_READY_ENDING; }
    // ReadyEnding → Running | Ended
    if (fromS == S_READY_ENDING) { return toS == S_RUNNING || toS == S_ENDED; }
    // Ended → (none)
    return false;
}
