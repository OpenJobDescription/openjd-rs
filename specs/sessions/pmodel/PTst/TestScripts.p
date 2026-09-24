/*****************************************************************************
 * TestScripts.p — test cases wiring drivers + spec monitors together.
 *
 * Run a single case:   p check -tc tcWorkerAgentSameUser
 * Run all:             p check
 *
 * Each module literal lists every machine that can be instantiated during the
 * run (listing an unused machine is harmless). Specs are attached with
 * `assert <spec-list> in <module>`.
 *****************************************************************************/

/* ---- same-user worker-agent lifecycle ---- */
test tcWorkerAgentSameUser
    [main = SetupWorkerAgentSameUser]:
    assert SessionStateSpec, EnvStackSpec, ActionLivenessSpec, CancelDeliverySpec in
    { SetupWorkerAgentSameUser, WorkerAgentDriver, Session, Subprocess, CrossUserHelper };

machine SetupWorkerAgentSameUser {
    start state Init { entry { new WorkerAgentDriver(false); } }
}

/* ---- cross-user worker-agent lifecycle (routes through the helper) ---- */
test tcWorkerAgentCrossUser
    [main = SetupWorkerAgentCrossUser]:
    assert SessionStateSpec, EnvStackSpec, ActionLivenessSpec, CancelDeliverySpec in
    { SetupWorkerAgentCrossUser, WorkerAgentDriver, Session, Subprocess, CrossUserHelper };

machine SetupWorkerAgentCrossUser {
    start state Init { entry { new WorkerAgentDriver(true); } }
}

/* ---- cancel_action → brittle-session contract ---- */
test tcCancelBrittle
    [main = SetupCancel]:
    assert SessionStateSpec, EnvStackSpec, ActionLivenessSpec, CancelDeliverySpec in
    { SetupCancel, CancelDriver, Session, Subprocess, CrossUserHelper };

machine SetupCancel {
    start state Init { entry { new CancelDriver(false); } }
}

/* ---- external token cancel, SAME-user (delivered via the token; the
 * same-user subprocess awaits the token, so the cancel takes effect) ---- */
test tcExternalCancelSameUser
    [main = SetupExternalCancelSameUser]:
    assert SessionStateSpec, EnvStackSpec, ActionLivenessSpec, CancelDeliverySpec in
    { SetupExternalCancelSameUser, ExternalCancelDriver, Session, Subprocess, CrossUserHelper };

machine SetupExternalCancelSameUser {
    start state Init { entry { new ExternalCancelDriver(false); } }
}

/* ---- external token cancel, CROSS-user. A token-only cancel must still take
 * effect on the cross-user path (CancelDeliverySpec). This is the case that
 * turns red when CROSS_USER_HONORS_TOKEN_CANCEL is flipped off — the fault
 * injection that shows the spec is specific to this path. ---- */
test tcExternalCancelCrossUser
    [main = SetupExternalCancelCrossUser]:
    assert SessionStateSpec, EnvStackSpec, ActionLivenessSpec, CancelDeliverySpec in
    { SetupExternalCancelCrossUser, ExternalCancelDriver, Session, Subprocess, CrossUserHelper };

machine SetupExternalCancelCrossUser {
    start state Init { entry { new ExternalCancelDriver(true); } }
}

/* ---- cross-user helper token security ---- */
test tcHelperTokenSecurity
    [main = SetupHelperToken]:
    assert ActionLivenessSpec in
    { SetupHelperToken, HelperTokenDriver, CrossUserHelper };

machine SetupHelperToken {
    start state Init { entry { new HelperTokenDriver(); } }
}
