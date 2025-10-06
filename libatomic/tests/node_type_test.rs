//! Unit tests for NodeType enum

use libatomic::pristine::NodeType;

#[test]
fn test_node_type_enum_values() {
    // Verify the numeric representation of each variant
    assert_eq!(NodeType::Change as u8, 0);
    assert_eq!(NodeType::Tag as u8, 1);
}

#[test]
fn test_node_type_from_u8_valid() {
    // Test valid conversions
    assert_eq!(NodeType::from_u8(0), Some(NodeType::Change));
    assert_eq!(NodeType::from_u8(1), Some(NodeType::Tag));
}

#[test]
fn test_node_type_from_u8_invalid() {
    // Test invalid conversions return None
    assert_eq!(NodeType::from_u8(2), None);
    assert_eq!(NodeType::from_u8(255), None);
    assert_eq!(NodeType::from_u8(100), None);
}

#[test]
fn test_node_type_round_trip() {
    // Test that converting to u8 and back works
    let change = NodeType::Change;
    let change_u8 = change as u8;
    assert_eq!(NodeType::from_u8(change_u8), Some(change));

    let tag = NodeType::Tag;
    let tag_u8 = tag as u8;
    assert_eq!(NodeType::from_u8(tag_u8), Some(tag));
}

#[test]
fn test_node_type_equality() {
    // Test equality comparisons
    assert_eq!(NodeType::Change, NodeType::Change);
    assert_eq!(NodeType::Tag, NodeType::Tag);
    assert_ne!(NodeType::Change, NodeType::Tag);
    assert_ne!(NodeType::Tag, NodeType::Change);
}

#[test]
fn test_node_type_clone() {
    // Test that NodeType is Clone
    let change = NodeType::Change;
    let change_clone = change;
    assert_eq!(change, change_clone);

    let tag = NodeType::Tag;
    let tag_clone = tag;
    assert_eq!(tag, tag_clone);
}

#[test]
fn test_node_type_debug() {
    // Test Debug formatting
    let change = NodeType::Change;
    let debug_str = format!("{:?}", change);
    assert!(debug_str.contains("Change"));

    let tag = NodeType::Tag;
    let debug_str = format!("{:?}", tag);
    assert!(debug_str.contains("Tag"));
}
