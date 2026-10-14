/*****************************************************************************
 * ActionLivenessSpec.p — every started action eventually terminates
 * (specs/sessions/session.md: no recovery path; subprocess always exits via
 *  normal exit, timeout→SIGKILL, or cancel→grace→SIGKILL).
 *
 * Liveness: whenever an action is Running (Busy), the system must eventually
 * reach a state where no action is Running (Idle).
 *****************************************************************************/

spec ActionLivenessSpec observes eMonActionStart, eMonActionEnd {
    start state Idle {
        on eMonActionStart goto Busy;
        // A spurious end without a start is caught by SessionStateSpec.
        ignore eMonActionEnd;
    }

    hot state Busy {
        on eMonActionEnd goto Idle;
        // Nested starts can't happen (SessionStateSpec enforces ≤1); ignore
        // defensively so this spec stays focused on the liveness question.
        ignore eMonActionStart;
    }
}
