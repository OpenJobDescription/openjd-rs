// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Edit distance computation for "Did you mean?" suggestions.

/// Levenshtein edit distance between two strings.
/// Two-row dynamic programming implementation.
pub fn edit_distance(s1: &str, s2: &str) -> usize {
    let s1: Vec<char> = s1.chars().collect();
    let s2: Vec<char> = s2.chars().collect();
    if s1.is_empty() {
        return s2.len();
    }
    if s2.is_empty() {
        return s1.len();
    }

    let mut prev: Vec<usize> = (0..=s2.len()).collect();
    let mut curr = vec![0; s2.len() + 1];

    for i in 1..=s1.len() {
        curr[0] = i;
        for j in 1..=s2.len() {
            let del = prev[j] + 1;
            let ins = curr[j - 1] + 1;
            let sub = prev[j - 1] + if s1[i - 1] == s2[j - 1] { 0 } else { 1 };
            curr[j] = del.min(ins).min(sub);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[s2.len()]
}

/// Maximum edit distance for a suggestion to be shown.
const MAX_SUGGESTION_DISTANCE: usize = 5;

/// Find the closest matching symbols to `name` from `available`.
/// Returns a formatted suggestion string, or empty string if no close match.
pub fn suggest_closest(name: &str, available: &[&str]) -> String {
    let mut best_dist = MAX_SUGGESTION_DISTANCE;
    let mut best: Vec<&str> = Vec::new();

    for &sym in available {
        let d = edit_distance(sym, name);
        if d < best_dist {
            best_dist = d;
            best = vec![sym];
        } else if d == best_dist {
            best.push(sym);
        }
    }

    if best.is_empty() {
        String::new()
    } else if best.len() == 1 {
        format!(" Did you mean: {}", best[0])
    } else {
        best.sort();
        format!(" Did you mean one of: {}", best.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical() {
        assert_eq!(edit_distance("abc", "abc"), 0);
    }

    #[test]
    fn test_empty() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", ""), 0);
    }

    #[test]
    fn test_single_insert() {
        assert_eq!(edit_distance("abc", "abcd"), 1);
    }

    #[test]
    fn test_single_delete() {
        assert_eq!(edit_distance("abcd", "abc"), 1);
    }

    #[test]
    fn test_single_substitute() {
        assert_eq!(edit_distance("abc", "axc"), 1);
    }

    #[test]
    fn test_typo() {
        assert_eq!(edit_distance("Param.Frame", "Param.Frane"), 1);
    }

    #[test]
    fn test_suggest_single_match() {
        let s = suggest_closest(
            "Param.Frane",
            &["Param.Frame", "Param.Scene", "RawParam.Frame"],
        );
        assert_eq!(s, " Did you mean: Param.Frame");
    }

    #[test]
    fn test_suggest_multiple_matches() {
        let s = suggest_closest("x", &["a", "b"]);
        // Both are distance 1 from "x"
        assert_eq!(s, " Did you mean one of: a, b");
    }

    #[test]
    fn test_suggest_no_close_match() {
        let s = suggest_closest("CompletelyDifferent", &["Param.Frame"]);
        assert_eq!(s, "");
    }

    #[test]
    fn test_suggest_empty_available() {
        assert_eq!(suggest_closest("anything", &[]), "");
    }
}
