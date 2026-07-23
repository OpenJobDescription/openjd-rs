// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/_internal/test_range_expr.py

use openjd_expr::range_expr::{IntRange, RangeExpr};

// ══════════════════════════════════════════════════════════════
// TestRangeExpressionParser — error cases
// ══════════════════════════════════════════════════════════════

#[test]
fn parse_empty_str() {
    assert!("".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_non_valid_char_at_end() {
    assert!("1-a".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_non_valid_char_at_start() {
    assert!("b-1".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_missing_start() {
    assert!("--12".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_missing_end() {
    assert!("1-".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_missing_end_in_first_range() {
    assert!("1-,3-4".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_missing_end_in_second_range() {
    assert!("1-10,11-".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_extra_hyphen_front_of_start() {
    assert!("--1-10".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_extra_hyphen_front_of_end() {
    assert!("1---10".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_extra_hyphen_front_of_step() {
    assert!("1--10:--5".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_extra_comma_between_ranges() {
    assert!("1-2,,3-4".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_trailing_comma() {
    assert!("1-:2,".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_missing_step() {
    assert!("1-2:".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_trailing_colon() {
    assert!("2:".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_leading_colon() {
    assert!(":2".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_just_a_colon() {
    assert!(":".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_missing_comma_between_ranges() {
    assert!("1-2 4-5".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_extra_number_at_end() {
    assert!("1-24-5".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_step_cannot_be_zero() {
    assert!("1-2:0".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_descending_range_positive_step() {
    // "2-0:1" — descending range with positive step
    assert!("2-0:1".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_ascending_range_negative_step() {
    // "0-1:-1" — ascending range with negative step
    assert!("0-1:-1".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_overlapping_complete() {
    assert!("1-10,1-10".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_overlapping_descending() {
    assert!("10-1:-1,10-1:-1".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_overlapping_negative() {
    assert!("-10--1,-10--1".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_overlapping_partial() {
    assert!("1-10,9-19".parse::<RangeExpr>().is_err());
}

#[test]
fn parse_overlapping_ascending_descending() {
    assert!("1-10:1,10-1:-1".parse::<RangeExpr>().is_err());
}

// ══════════════════════════════════════════════════════════════
// TestRangeExpressionParser — single value
// ══════════════════════════════════════════════════════════════

#[test]
fn parse_only_start_negative() {
    let r = "-9999999".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r.get(0), Some(-9999999));
}

#[test]
fn parse_only_start_zero() {
    let r = "0".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r.get(0), Some(0));
}

#[test]
fn parse_only_start_positive() {
    let r = "100".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r.get(0), Some(100));
}

// ══════════════════════════════════════════════════════════════
// TestRangeExpressionParser — whitespace
// ══════════════════════════════════════════════════════════════

#[test]
fn parse_ignore_whitespace() {
    let with_ws = "\t0 - 1 :\t1, 2 -\t100 : 1".parse::<RangeExpr>().unwrap();
    let without_ws = "0-1:1,2-100:1".parse::<RangeExpr>().unwrap();
    assert_eq!(with_ws.to_vec(), without_ws.to_vec());
}

// ══════════════════════════════════════════════════════════════
// TestRangeExpressionParser — positive range no step
// ══════════════════════════════════════════════════════════════

#[test]
fn parse_one_positive_range_no_step() {
    let r = "1-100".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 100);
    assert_eq!(r.get(0), Some(1));
    assert_eq!(r.get(99), Some(100));
}

// ══════════════════════════════════════════════════════════════
// TestRangeExpressionParser — multiple non-overlapping ranges
// ══════════════════════════════════════════════════════════════

#[test]
fn parse_multiple_contiguous() {
    let r = "1-100,101-200".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 200);
    assert_eq!(r.to_string(), "1-200");
}

#[test]
fn parse_multiple_with_gaps() {
    let r = "0-1,3-4,7-9,10".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 8);
    assert_eq!(r.to_string(), "0,1,3,4,7-10");
}

#[test]
fn parse_multiple_with_steps() {
    let r = "0-3:3,5-10:5,12,13,14,15".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 8);
    assert_eq!(r.to_string(), "0,3,5,10,12-15");
}

#[test]
fn parse_out_of_order() {
    let r = "20-29,0-9,10-19".parse::<RangeExpr>().unwrap();
    assert_eq!(r.len(), 30);
    assert_eq!(r.to_string(), "0-29");
}

// ══════════════════════════════════════════════════════════════
// TestIntRangeExpr — sorting and merging
// ══════════════════════════════════════════════════════════════

#[test]
fn range_sorting_and_merging() {
    let first = IntRange::new(-9, 0, 1).unwrap();
    let second = IntRange::new(1, 10, 1).unwrap();
    let third = IntRange::new(11, 20, 1).unwrap();
    let r = RangeExpr::from_ranges(vec![third, second, first]).unwrap();
    assert_eq!(r.ranges(), &[IntRange::new(-9, 20, 1).unwrap()]);
    assert_eq!(r.to_string(), "-9-20");
}

#[test]
fn sorting_merging_with_descending_ranges() {
    let first = IntRange::new(-10, -19, -1).unwrap();
    let second = IntRange::new(-1, -9, -1).unwrap();
    let third = IntRange::new(10, 0, -1).unwrap();
    let r = RangeExpr::from_ranges(vec![second, third, first]).unwrap();
    assert_eq!(r.ranges(), &[IntRange::new(-19, 10, 1).unwrap()]);
}

#[test]
fn sorting_merging_mixed_ascending_descending() {
    let first = IntRange::new(-9, -7, 1).unwrap();
    let second = IntRange::new(1, -6, -1).unwrap();
    let third = IntRange::new(2, 9, 1).unwrap();
    let r = RangeExpr::from_ranges(vec![second, first, third]).unwrap();
    assert_eq!(r.ranges(), &[IntRange::new(-9, 9, 1).unwrap()]);
}

// ══════════════════════════════════════════════════════════════
// TestIntRangeExpr — from_str
// ══════════════════════════════════════════════════════════════

#[test]
fn from_str_one_int() {
    assert_eq!("  5 ".parse::<RangeExpr>().unwrap().to_string(), "5");
}

#[test]
fn from_str_out_of_order() {
    assert_eq!(
        "9,0,3,2,8,10,1,4,7,6,5"
            .parse::<RangeExpr>()
            .unwrap()
            .to_string(),
        "0-10"
    );
}

#[test]
fn from_str_ranges_out_of_order_with_steps() {
    assert_eq!(
        "3-5,0-2,8-12:2".parse::<RangeExpr>().unwrap().to_string(),
        "0-5,8-12:2"
    );
}

#[test]
fn from_str_pos_neg_steps() {
    assert_eq!(
        "5-3:-1,12-8:-2,2-0:-1,6-7:2"
            .parse::<RangeExpr>()
            .unwrap()
            .to_string(),
        "0-5,6-12:2"
    );
}

#[test]
fn from_str_end_becomes_actual_last_2_values() {
    assert_eq!("1-5:3".parse::<RangeExpr>().unwrap().to_string(), "1,4");
}

#[test]
fn from_str_end_included() {
    assert_eq!("1-7:3".parse::<RangeExpr>().unwrap().to_string(), "1-7:3");
}

#[test]
fn from_str_end_becomes_actual_last_3_values() {
    assert_eq!("1-9:3".parse::<RangeExpr>().unwrap().to_string(), "1-7:3");
}

// ══════════════════════════════════════════════════════════════
// TestIntRangeExpr — from_values (from_list in Python)
// ══════════════════════════════════════════════════════════════

#[test]
fn from_values_one_int() {
    assert_eq!(RangeExpr::from_values(vec![5]).unwrap().to_string(), "5");
}

#[test]
fn from_values_two_ranges() {
    assert_eq!(
        RangeExpr::from_values(vec![1, 2, 3, 4, 5, 7])
            .unwrap()
            .to_string(),
        "1-5,7"
    );
}

#[test]
fn from_values_out_of_order() {
    assert_eq!(
        RangeExpr::from_values(vec![9, 0, 3, 2, 8, 10, 1, 4, 7, 6, 5])
            .unwrap()
            .to_string(),
        "0-10"
    );
}

#[test]
fn from_values_different_step_sizes() {
    assert_eq!(
        RangeExpr::from_values(vec![1, 3, 5, 6, 7, 8, 10, 13, 16])
            .unwrap()
            .to_string(),
        "1-5:2,6-8,10-16:3"
    );
}

// ══════════════════════════════════════════════════════════════
// TestIntRangeExpr — getitem
// ══════════════════════════════════════════════════════════════

fn make_test_range_expr() -> RangeExpr {
    let first = IntRange::new(-5, 0, 1).unwrap();
    let second = IntRange::new(5, 10, 1).unwrap();
    let third = IntRange::new(13, 20, 1).unwrap();
    RangeExpr::from_ranges(vec![third, second, first]).unwrap()
}

#[test]
fn getitem_first_range_first() {
    assert_eq!(make_test_range_expr().get(0), Some(-5));
}

#[test]
fn getitem_first_range_middle() {
    assert_eq!(make_test_range_expr().get(2), Some(-3));
}

#[test]
fn getitem_second_range() {
    // Index 6 = first 6 elements (-5..0) then 5
    assert_eq!(make_test_range_expr().get(6), Some(5));
}

#[test]
fn getitem_third_range() {
    // first range: 6 elements (-5..0), second: 6 (5..10), third starts at index 12
    assert_eq!(make_test_range_expr().get(15), Some(16));
}

#[test]
fn getitem_last() {
    assert_eq!(make_test_range_expr().get(19), Some(20));
}

#[test]
fn getitem_negative_first() {
    assert_eq!(make_test_range_expr().get(-1), Some(20));
}

#[test]
fn getitem_negative_last() {
    assert_eq!(make_test_range_expr().get(-20), Some(-5));
}

#[test]
fn getitem_out_of_bounds_positive() {
    assert_eq!(make_test_range_expr().get(20), None);
    assert_eq!(make_test_range_expr().get(100), None);
}

#[test]
fn getitem_out_of_bounds_negative() {
    assert_eq!(make_test_range_expr().get(-21), None);
    assert_eq!(make_test_range_expr().get(-101), None);
}

// ══════════════════════════════════════════════════════════════
// TestIntRangeExpr — iteration
// ══════════════════════════════════════════════════════════════

#[test]
fn iterable() {
    let r = "1-10,11-20".parse::<RangeExpr>().unwrap();
    let expected: Vec<i64> = (1..=20).collect();
    let actual: Vec<i64> = r.iter().collect();
    assert_eq!(actual, expected);
}

// ══════════════════════════════════════════════════════════════
// TestIntRangeExpr — contains
// ══════════════════════════════════════════════════════════════

#[test]
fn contains() {
    let r = "0-10".parse::<RangeExpr>().unwrap();
    assert!(r.contains(0));
    assert!(r.contains(10));
    assert!(!r.contains(-1));
    assert!(!r.contains(11));
}

#[test]
fn contains_negative_descending() {
    let r = "-1--2:-1".parse::<RangeExpr>().unwrap();
    assert!(r.contains(-1));
    assert!(r.contains(-2));
    assert!(!r.contains(-3));
    assert!(!r.contains(0));
}

// ══════════════════════════════════════════════════════════════
// TestIntRange — length
// ══════════════════════════════════════════════════════════════

#[test]
fn int_range_length() {
    // positive ascending
    assert_eq!(IntRange::new(0, 0, 1).unwrap().len(), 1);
    assert_eq!(IntRange::new(0, 10, 1).unwrap().len(), 11);
    assert_eq!(IntRange::new(0, 10, 2).unwrap().len(), 6);

    // negative ascending
    assert_eq!(IntRange::new(-3, -1, 1).unwrap().len(), 3);
    assert_eq!(IntRange::new(-3, 1, 1).unwrap().len(), 5);
    assert_eq!(IntRange::new(-3, 1, 2).unwrap().len(), 3);

    // positive descending
    assert_eq!(IntRange::new(0, 0, -1).unwrap().len(), 1);
    assert_eq!(IntRange::new(10, -5, -1).unwrap().len(), 16);
    assert_eq!(IntRange::new(10, -5, -2).unwrap().len(), 8);

    // negative descending
    assert_eq!(IntRange::new(-3, -7, -1).unwrap().len(), 5);
    assert_eq!(IntRange::new(-3, -7, -2).unwrap().len(), 3);
}

#[test]
fn int_range_display() {
    assert_eq!(format!("{}", IntRange::new(1, 10, 1).unwrap()), "1-10");
}

#[test]
fn int_range_getitem_out_of_bounds() {
    let r = IntRange::new(0, 5, 1).unwrap();
    assert_eq!(r.get(100), None);
}
