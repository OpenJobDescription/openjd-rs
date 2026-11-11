// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! PID-reuse-safe process tree collection for Windows tree kills.
//!
//! Both Windows tree kills use this: the in-process one in `openjd-sessions`'
//! `subprocess.rs` and the cross-user helper's in `runner_win.rs`. Each one
//! takes a toolhelp snapshot of `(pid, th32ParentProcessID)` pairs, reads
//! process creation times, and terminates what this module accepts. Keeping
//! the edge validation in one file means a fix to it reaches both kills;
//! when each crate had its own copy, the PID-reuse fix reached only one.
//!
//! Deliberately `std` only and free of Windows calls: the caller supplies the
//! snapshot and a creation-time callback, so the logic is unit-tested on
//! every platform. It compiles into the standalone helper binary (a nested
//! crate that cannot depend on `openjd-sessions`) and is `#[path]`-included
//! into `openjd-sessions`, so it references nothing from either crate.

/// A process node whose creation time was successfully read and validated
/// against its parent during tree collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedProcess {
    /// The process ID.
    pub pid: u32,
    /// The creation time the node was validated with, in `FILETIME` ticks.
    pub creation_time: u64,
}

/// Collect the process tree rooted at `root_pid` in breadth-first order,
/// validating every parent->child edge against process creation times.
///
/// Walks the `parents` snapshot (a list of `(pid, ppid)` pairs) with a
/// visited set instead of recursing. Both details matter:
/// `th32ParentProcessID` is recorded at child creation time, so PID reuse
/// can make the recorded parent graph cyclic (a dead ancestor's PID reused
/// by a descendant). The previous recursive implementation had no cycle
/// guard and overflowed the thread's stack (0xc00000fd) when it hit such a
/// cycle.
///
/// On top of the cycle guard this mirrors psutil's `Process.children()`
/// PID-reuse guard, which the original Python->Rust port dropped: under a
/// forward-moving system clock, a true child is never older than its
/// parent, so an edge (child, ppid) is accepted only when `ppid` is an
/// already-accepted node AND the child's creation time is `>=` that
/// parent's. A stale edge (the parent PID was reused after the real parent
/// exited) has a child older than the recorded parent and always fails the
/// check. We fail closed on stale edges (a child older than its recorded
/// parent, the mark of a reused parent PID). A child whose creation time is
/// unreadable is NOT skipped: it is recorded as a traversable node carrying
/// the parent's creation time as its bound (see the later paragraph), so a
/// live subtree under a dead intermediate is still reached. If the ROOT's
/// creation time is unreadable we return an empty `Vec` ONLY when no
/// schedule-time identity is pinned; with `Some(exp)` the walk is seeded from
/// that pinned identity and proceeds.
///
/// Because `FILETIME` creation times are wall-clock rather than monotonic,
/// this invariant is only exact under a forward-moving clock. An NTP
/// step-back or manual clock change between a parent's and its child's
/// creation can make a TRUE child's recorded creation time earlier than its
/// parent's, failing the `>=` edge check and pruning that child's whole
/// subtree. The failure mode is a leaked (skipped) descendant, never
/// terminating an unrelated process, so this stays on the safe under-kill
/// side.
///
/// `snapshot_time` closes a second PID-reuse direction: the one where a
/// genuine edge (child, parent) is captured in the snapshot, the child then
/// exits, and its PID is reused by an unrelated process U before this function
/// reads the child's creation time. U's creation time is newer than the
/// parent's, so the `>=` edge check passes and U would be recorded as a
/// validated node. Because every process the snapshot lists was created before
/// the snapshot was taken (the caller captures the stamp right after
/// `CreateToolhelp32Snapshot` returns), we reject any DESCENDANT candidate whose freshly read creation time is greater
/// than `snapshot_time`. The stamp and process creation times share the same
/// wall-clock base (FILETIME ticks since 1601 UTC), so the comparison is exact.
///
/// The root is exempt from the `snapshot_time` bound in every arm. The root's
/// identity is never established by fossil-edge attribution (the reuse the
/// stamp guards): it is either pinned by a live `Child` handle
/// (`expected_root_ct = None`: `subprocess.rs`'s immediate `send_terminate`
/// and every kill in the cross-user helper), matched against a
/// schedule-time identity (`Some(exp)` with `read == exp`), or seeded from that
/// pinned identity when the root has already exited (`Some(exp)` with `None`).
/// A stamp comparison on the root could therefore never catch a reused PID; it
/// could only false-positive under a backward wall-clock adjustment (an NTP
/// step-back or a VM resume, where the root's FILETIME creation was recorded
/// before the step and the stamp read after it), return empty, and turn a
/// legitimate cancel into a complete no-op — nothing killed, the session left
/// waiting on a live tree. Accepted degradation: under such a backward step the
/// root is still killed, but descendants whose recorded creation times exceed
/// the post-step stamp may be skipped. That is an under-kill (a leaked
/// descendant), which stays on the safe side.
///
/// `expected_root_ct` pins the root's identity to a value established before
/// this call (a schedule-time read from `subprocess.rs`'s delayed-terminate
/// path). When
/// `Some(exp)`:
/// - a fresh root read of `Some(rc)` must equal `exp`, otherwise this returns
///   empty (fail closed) — closing the reuse window between an earlier identity
///   read and this walk;
/// - a fresh root read of `None` (the root exited during the grace period, its
///   PID currently unassigned) SEEDS the walk with `ValidatedProcess { pid:
///   root_pid, creation_time: exp }` and proceeds. This is the common
///   delayed-path case, whose whole purpose is reaping orphaned grandchildren:
///   `kill_process_checked` no-ops on the dead root, but its orphaned
///   descendants are still enumerated and validated against `>= exp` and
///   `<= snapshot_time`.
///
/// The recorded root node then carries `exp`, the pinned identity, not any
/// re-read value. When `expected_root_ct` is `None`, an unreadable fresh root
/// read returns empty (fail closed) and a readable one is accepted as-is (the
/// live `Child` handle already pins the PID; see the root-exemption note
/// above).
///
/// A child whose creation time is unreadable under an already-validated parent
/// is kept TRAVERSABLE rather than pruned: it is recorded carrying the parent's
/// creation time (a sound lower bound for ITS children) so that a live subtree
/// under a dead intermediate (cmd -> sh -> renderer, sh exits first) is still
/// reached. `kill_process_checked` cannot open the dead PID, so nothing is
/// terminated for the node itself; only its readable, validated descendants
/// are. Stale (`< parent`) and post-snapshot (`> snapshot_time`) rejections
/// still apply to READABLE children exactly as before.
///
/// Residual: seeding a dead root and traversing dead intermediates reopens a
/// bounded window. A freed PID's fossil edges (recorded `th32ParentProcessID`
/// values) can include children of an INTERIM holder of that PID whose creation
/// times happen to land in `[exp, snapshot_time]`; those pass validation and
/// may be terminated. The window is bounded and no worse than the
/// pre-validation walk, and is structurally eliminated only by the Job Object
/// rework (issue #347).
///
/// Separate residual (distinct from the dead-root/interim-holder case above):
/// an unreadable-but-ALIVE fossil child. If a recorded `th32ParentProcessID`
/// was freed and reassigned to OUR root, an access-denied process carrying
/// that stale parent value becomes a child edge of the root. Because it is
/// unreadable it bypasses the `>=` staleness check by construction (that check
/// only applies to READABLE children) and is recorded as a traversable node
/// carrying the parent's bound, so its own live children can fall inside
/// `[parent_ct, snapshot_time]` and be terminated. Distinguishing genuinely
/// dead PIDs (`OpenProcess` -> `ERROR_INVALID_PARAMETER`) from merely denied
/// ones (`ERROR_ACCESS_DENIED`) would close this, but that changes the
/// creation-time callback contract, so it is deferred with the Job Object
/// rework (issue #347).
pub fn collect_tree_validated(
    root_pid: u32,
    parents: &[(u32, u32)],
    snapshot_time: u64,
    expected_root_ct: Option<u64>,
    creation_time: &mut dyn FnMut(u32) -> Option<u64>,
) -> Vec<ValidatedProcess> {
    use std::collections::HashSet;

    // Resolve the root's identity. The four cases below implement the pinned
    // vs. unpinned, readable vs. exited matrix documented above.
    let root_ct = match (expected_root_ct, creation_time(root_pid)) {
        // Pinned identity, root still readable: the fresh read must match the
        // pin exactly (else reuse/exit). No snapshot_time bound — the identity
        // match already proves this is the pinned process, so a stamp
        // comparison here could only false-positive under a backward clock
        // step-back and turn a legitimate cancel into a no-op.
        //
        // On mismatch we deliberately ABORT (return empty) rather than seed the
        // walk. If `rc != exp` the PID has been REASSIGNED: the new occupant is
        // alive, and its children are guaranteed-live unrelated processes whose
        // creation times necessarily land in `[exp, snapshot_time]`, so they
        // would pass the `>=`/`<=` edge checks. Seeding a root here would
        // therefore deterministically kill those unrelated processes. Aborting
        // instead only leaks our own orphans (under-kill), which is the safe
        // direction.
        (Some(exp), Some(rc)) => {
            if rc != exp {
                return Vec::new();
            }
            exp
        }
        // Pinned identity, root exited during the grace period: seed with the
        // pinned identity so orphaned descendants are still reaped.
        (Some(exp), None) => exp,
        // No pinned identity and no fresh read: nothing to validate against.
        (None, None) => return Vec::new(),
        // No pinned identity, root readable: every caller passing None (the
        // immediate send_terminate path and the cross-user helper) holds the
        // workload's live Child handle, which pins the PID, so rc cannot
        // belong to a reused PID. No snapshot_time bound — a
        // stamp comparison here can only false-positive under a backward clock
        // adjustment and would turn a legitimate cancel into a no-op.
        (None, Some(rc)) => rc,
    };

    let mut visited: HashSet<u32> = HashSet::from([root_pid]);
    let mut result: Vec<ValidatedProcess> = vec![ValidatedProcess {
        pid: root_pid,
        creation_time: root_ct,
    }];

    let mut i = 0;
    while i < result.len() {
        let parent = result[i];
        i += 1;
        for &(child, ppid) in parents {
            // Only descend from the node we are currently processing, and
            // visit each child once (this is the cycle/duplicate guard).
            if ppid != parent.pid || !visited.insert(child) {
                continue;
            }
            match creation_time(child) {
                Some(child_ct) => {
                    // A true child is never older than its parent, and — like
                    // every genuine snapshot member — was created before the
                    // snapshot. A creation time newer than the snapshot marks a
                    // PID reused after the snapshot (fail closed); an
                    // older-than-parent one marks a stale reused-parent edge.
                    if child_ct >= parent.creation_time && child_ct <= snapshot_time {
                        result.push(ValidatedProcess {
                            pid: child,
                            creation_time: child_ct,
                        });
                    }
                }
                None => {
                    // Unreadable child under a validated parent: the node
                    // itself cannot be opened (kill_process_checked no-ops on
                    // it), but it may still have live descendants. Keep it
                    // traversable carrying the parent's creation time — a sound
                    // lower bound for ITS children — instead of pruning the
                    // whole subtree. See the interim-holder residual note above.
                    result.push(ValidatedProcess {
                        pid: child,
                        creation_time: parent.creation_time,
                    });
                }
            }
        }
    }
    result
}

/// Kill-time revalidation predicate guarding the TOCTOU window between tree
/// collection and `TerminateProcess`: a PID collected earlier could be
/// reused by an unrelated process before we terminate it. Returns `true`
/// only when the process currently at that PID reports exactly the creation
/// time recorded at collection.
pub fn process_identity_matches(expected_creation_time: u64, current: Option<u64>) -> bool {
    current == Some(expected_creation_time)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `creation_time` callback from a slice of `(pid, ctime)` pairs.
    /// Any pid absent from the slice reports an unreadable creation time.
    fn ctimes_from(pairs: &[(u32, u64)]) -> impl FnMut(u32) -> Option<u64> + '_ {
        move |pid| pairs.iter().find(|(p, _)| *p == pid).map(|(_, ct)| *ct)
    }

    fn vp(pid: u32, creation_time: u64) -> ValidatedProcess {
        ValidatedProcess { pid, creation_time }
    }

    #[test]
    fn collects_true_descendants_in_bfs_order() {
        let parents = [(20u32, 10u32), (30, 20)];
        let mut ct = ctimes_from(&[(10, 100), (20, 200), (30, 300)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 200), vp(30, 300)]);
    }

    #[test]
    fn rejects_stale_edge_child_older_than_parent() {
        let parents = [(40u32, 10u32)];
        let mut ct = ctimes_from(&[(10, 500), (40, 100)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 500)]);
    }

    #[test]
    fn unreadable_child_stays_traversable_with_parent_bound() {
        // A child whose creation time is unreadable under a validated parent is
        // no longer pruned: it is recorded carrying the parent's creation time
        // so its own descendants remain reachable. kill_process_checked will
        // no-op on the unreadable node at kill time.
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(10, 100)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 100)]);
    }

    #[test]
    fn unreadable_intermediate_keeps_live_grandchild() {
        // cmd(10) -> sh(20, exited/unreadable) -> renderer(30, live). The dead
        // intermediate must stay traversable carrying its parent's bound (100)
        // so the live grandchild is still enumerated and validated.
        let parents = [(20u32, 10u32), (30, 20)];
        let mut ct = ctimes_from(&[(10, 100), (30, 300)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 100), vp(30, 300)]);
    }

    #[test]
    fn unreadable_intermediate_bound_still_prunes_stale_grandchild() {
        // Same shape, but the grandchild reads OLDER than the inherited parent
        // bound (50 < 100): it is a stale reused-parent edge and is rejected.
        let parents = [(20u32, 10u32), (30, 20)];
        let mut ct = ctimes_from(&[(10, 100), (30, 50)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 100)]);
    }

    #[test]
    fn dead_root_with_pinned_identity_reaps_orphans() {
        // Root(10) exited during the grace period (unreadable) but a
        // schedule-time identity (200) was pinned. Seed the walk with the pin
        // and reap the orphaned descendant(20).
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(20, 250)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, Some(200), &mut ct);
        assert_eq!(got, vec![vp(10, 200), vp(20, 250)]);
    }

    #[test]
    fn returns_empty_when_root_ctime_unreadable() {
        // Unpinned (expected_root_ct = None) root that is unreadable: nothing to
        // validate against, so fail closed. Contrast dead_root_with_pinned_
        // identity_reaps_orphans, where a pinned identity seeds the walk.
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(20, 200)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, Vec::<ValidatedProcess>::new());
    }

    #[test]
    fn stale_cycle_terminates() {
        let parents = [(20u32, 10u32), (10, 20)];
        let mut ct = ctimes_from(&[(10, 500), (20, 100)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 500)]);
    }

    #[test]
    fn accepts_equal_creation_time() {
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(10, 100), (20, 100)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 100)]);
    }

    #[test]
    fn prunes_entire_subtree_below_rejected_edge() {
        let parents = [(20u32, 10u32), (30, 20)];
        let mut ct = ctimes_from(&[(10, 500), (20, 100), (30, 600)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 500)]);
    }

    #[test]
    fn equal_time_cycle_terminates_without_duplicates() {
        let parents = [(20u32, 10u32), (10, 20)];
        let mut ct = ctimes_from(&[(10, 100), (20, 100)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 100)]);
    }

    #[test]
    fn identity_match_rejects_changed_creation_time() {
        assert!(!process_identity_matches(200, Some(900)));
        assert!(!process_identity_matches(200, None));
    }

    #[test]
    fn identity_match_accepts_same_creation_time() {
        assert!(process_identity_matches(200, Some(200)));
    }

    #[test]
    fn rejects_candidate_created_after_snapshot() {
        // Finding 1: the edge (20,10) is genuine at snapshot time, but pid 20
        // exited and was reused by a newer process. snapshot_time bounds out
        // the impostor even though ctime 500 passes the `>=` parent check.
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(10, 100), (20, 500)]);
        let got = collect_tree_validated(10, &parents, 300, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100)]);
    }

    #[test]
    fn immediate_root_newer_than_snapshot_still_collected() {
        // Immediate send_terminate path (expected None): the live Child handle
        // pins the PID, so the root is exempt from the snapshot bound even
        // though its ctime (900) post-dates the stamp (300). A descendant that
        // also post-dates the stamp (950) is still rejected.
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(10, 900), (20, 950)]);
        let got = collect_tree_validated(10, &parents, 300, None, &mut ct);
        assert_eq!(got, vec![vp(10, 900)]);
    }

    #[test]
    fn pinned_root_newer_than_snapshot_still_collected() {
        // Delayed path with a matching pin: the identity match proves the
        // pinned process, so the root is collected despite ctime (900) > stamp
        // (300).
        let parents: [(u32, u32); 0] = [];
        let mut ct = ctimes_from(&[(10, 900)]);
        let got = collect_tree_validated(10, &parents, 300, Some(900), &mut ct);
        assert_eq!(got, vec![vp(10, 900)]);
    }

    #[test]
    fn root_exemption_does_not_extend_to_children() {
        // The root (100) is exempt from the stamp (300), but children still
        // obey it: 250 is accepted, 400 is rejected as a post-snapshot reuse.
        let parents = [(20u32, 10u32), (30, 10)];
        let mut ct = ctimes_from(&[(10, 100), (20, 250), (30, 400)]);
        let got = collect_tree_validated(10, &parents, 300, None, &mut ct);
        assert_eq!(got, vec![vp(10, 100), vp(20, 250)]);
    }

    #[test]
    fn root_expected_ctime_mismatch_returns_empty() {
        // Finding 2: pinned schedule-time identity 200, but the root now reads
        // 900 => reused or exited => fail closed.
        let parents = [(20u32, 10u32)];
        let mut ct = ctimes_from(&[(10, 900), (20, 950)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, Some(200), &mut ct);
        assert_eq!(got, Vec::<ValidatedProcess>::new());
    }

    #[test]
    fn root_expected_ctime_match_uses_expected_identity() {
        // Pinned identity 200 matches the fresh read; the recorded root carries
        // the pinned value and children are collected normally.
        let parents = [(20u32, 10u32), (30, 20)];
        let mut ct = ctimes_from(&[(10, 200), (20, 300), (30, 400)]);
        let got = collect_tree_validated(10, &parents, u64::MAX, Some(200), &mut ct);
        assert_eq!(got, vec![vp(10, 200), vp(20, 300), vp(30, 400)]);
        assert_eq!(got[0].creation_time, 200);
    }
}
