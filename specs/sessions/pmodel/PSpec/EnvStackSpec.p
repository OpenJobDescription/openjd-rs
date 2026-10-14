/*****************************************************************************
 * EnvStackSpec.p — environment LIFO + no-duplicate invariants
 * (specs/sessions/session.md § Environment Management, § Why LIFO enforcement).
 *
 * Asserts:
 *   - environments are popped in strict LIFO order (last pushed = first popped)
 *   - no duplicate environment id is ever on the stack simultaneously
 *   - cleanup() with a non-empty stack is flagged (onExit scripts skipped)
 *****************************************************************************/

spec EnvStackSpec observes eMonEnter, eMonExit, eMonCleanup {
    var stack: seq[tEnvId];

    start state Watching {
        entry { stack = default(seq[tEnvId]); }

        on eMonEnter do (id: tEnvId) {
            assert !contains(id), format ("Duplicate environment {0} pushed onto stack", id);
            stack += (sizeof(stack), id);
        }

        on eMonExit do (id: tEnvId) {
            assert sizeof(stack) > 0, "Exit with empty environment stack";
            assert stack[sizeof(stack) - 1] == id,
                format ("Non-LIFO exit: popped {0} but top is {1}", id, stack[sizeof(stack) - 1]);
            stack -= (sizeof(stack) - 1);
        }

        on eMonCleanup do (depth: int) {
            assert depth == sizeof(stack), "monitor/session stack depth diverged";
            // Documented hazard, not a correctness bug in the model, but a
            // well-formed driver should have exited everything first.
            assert depth == 0,
                format ("cleanup() called with {0} environment(s) still entered", depth);
        }
    }

    fun contains(id: tEnvId): bool {
        var i: int;
        i = 0;
        while (i < sizeof(stack)) {
            if (stack[i] == id) { return true; }
            i = i + 1;
        }
        return false;
    }
}
