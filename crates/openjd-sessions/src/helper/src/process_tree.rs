// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! PID-reuse-safe process tree collection for both Windows tree kills
//! (`openjd-sessions`' `subprocess.rs` and the helper's `runner_win.rs`).
//!
//! `std` only and free of Windows calls, so its unit tests run everywhere,
//! and it references nothing from either crate, because `openjd-sessions`
//! `#[path]`-includes it. See specs/sessions/subprocess.md § Process Tree
//! Killing and § Shared implementation.

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
/// validating every parent->child edge in `parents` (`(pid, ppid)` pairs)
/// against creation times.
///
/// `snapshot_time` is the stamp taken when `parents` was captured, and
/// `expected_root_ct` an optional pinned root identity. Returns empty when the
/// root's identity can't be established. The rules are in
/// specs/sessions/subprocess.md § Root identity, § Edge validation and
/// § Unreadable children.
pub fn collect_tree_validated(
    root_pid: u32,
    parents: &[(u32, u32)],
    snapshot_time: u64,
    expected_root_ct: Option<u64>,
    creation_time: &mut dyn FnMut(u32) -> Option<u64>,
) -> Vec<ValidatedProcess> {
    use std::collections::HashSet;

    // The four cases are the table in specs/sessions/subprocess.md § Root
    // identity. The root is exempt from `snapshot_time` in every case.
    let root_ct = match (expected_root_ct, creation_time(root_pid)) {
        // A mismatch means the PID was reassigned. Abort rather than seed:
        // the new occupant's children would pass every edge check.
        (Some(exp), Some(rc)) => {
            if rc != exp {
                return Vec::new();
            }
            exp
        }
        // The pinned root exited: seed with the pin to reap its orphans.
        (Some(exp), None) => exp,
        (None, None) => return Vec::new(),
        // The caller's live `Child` handle pins the PID.
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
            // `visited` is the cycle and duplicate guard.
            if ppid != parent.pid || !visited.insert(child) {
                continue;
            }
            match creation_time(child) {
                Some(child_ct) => {
                    // Older than the parent: stale edge. Newer than the
                    // snapshot: PID reused after it. Either way, prune.
                    if child_ct >= parent.creation_time && child_ct <= snapshot_time {
                        result.push(ValidatedProcess {
                            pid: child,
                            creation_time: child_ct,
                        });
                    }
                }
                None => {
                    // Keep it traversable with the parent's time as the bound
                    // for its children; the kill-time check skips it itself.
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

/// Whether the process now at a collected PID is the one collected: `true`
/// only when its `current` creation time equals the recorded one. See
/// specs/sessions/subprocess.md § Kill-time revalidation.
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
