use super::*;

#[test]
fn empty_get_returns_none() {
    let tree: ARTMap<i32> = ARTMap::new();
    assert!(tree.get(b"anything").is_none());
}

#[test]
fn empty_len_is_zero() {
    let tree: ARTMap<i32> = ARTMap::new();
    assert_eq!(tree.len(), 0);
    assert!(tree.is_empty());
}

#[test]
fn put_and_get_single() {
    let mut tree = ARTMap::new();
    tree.put(b"hello", 42);
    assert_eq!(tree.get(b"hello"), Some(&42));
    assert_eq!(tree.len(), 1);
}

#[test]
fn put_overwrite() {
    let mut tree = ARTMap::new();
    tree.put(b"k", 1);
    tree.put(b"k", 2);
    assert_eq!(tree.get(b"k"), Some(&2));
    assert_eq!(tree.len(), 1);
}

#[test]
fn get_missing() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    assert!(tree.get(b"b").is_none());
    assert!(tree.get(b"ab").is_none());
}

#[test]
fn multiple_independent_keys() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    tree.put(b"c", 3);
    assert_eq!(tree.get(b"a"), Some(&1));
    assert_eq!(tree.get(b"b"), Some(&2));
    assert_eq!(tree.get(b"c"), Some(&3));
    assert_eq!(tree.len(), 3);
}

#[test]
fn shared_prefix() {
    let mut tree = ARTMap::new();
    tree.put(b"abc", 1);
    tree.put(b"abd", 2);
    tree.put(b"xyz", 3);
    assert_eq!(tree.get(b"abc"), Some(&1));
    assert_eq!(tree.get(b"abd"), Some(&2));
    assert_eq!(tree.get(b"xyz"), Some(&3));
}

#[test]
fn prefix_key_short_then_long() {
    let mut tree = ARTMap::new();
    tree.put(b"ab", 1);
    tree.put(b"abc", 2);
    assert_eq!(tree.get(b"ab"), Some(&1));
    assert_eq!(tree.get(b"abc"), Some(&2));
    assert_eq!(tree.len(), 2);
}

#[test]
fn prefix_key_long_then_short() {
    let mut tree = ARTMap::new();
    tree.put(b"abc", 2);
    tree.put(b"ab", 1);
    assert_eq!(tree.get(b"ab"), Some(&1));
    assert_eq!(tree.get(b"abc"), Some(&2));
    assert_eq!(tree.len(), 2);
}

#[test]
fn three_level_prefix_chain() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"ab", 2);
    tree.put(b"abc", 3);
    assert_eq!(tree.get(b"a"), Some(&1));
    assert_eq!(tree.get(b"ab"), Some(&2));
    assert_eq!(tree.get(b"abc"), Some(&3));
    assert_eq!(tree.len(), 3);
}

#[test]
fn empty_key() {
    let mut tree = ARTMap::new();
    tree.put(b"", 0);
    tree.put(b"a", 1);
    assert_eq!(tree.get(b""), Some(&0));
    assert_eq!(tree.get(b"a"), Some(&1));
}

#[test]
fn deep_shared_prefix() {
    let mut tree = ARTMap::new();
    tree.put(b"abcdefghij", 1);
    tree.put(b"abcdefghik", 2);
    assert_eq!(tree.get(b"abcdefghij"), Some(&1));
    assert_eq!(tree.get(b"abcdefghik"), Some(&2));
}

#[test]
fn prefix_split_later_insert() {
    let mut tree = ARTMap::new();
    tree.put(b"abcdef", 1);
    tree.put(b"abcxyz", 2);
    tree.put(b"abZZZ", 3);
    assert_eq!(tree.get(b"abcdef"), Some(&1));
    assert_eq!(tree.get(b"abcxyz"), Some(&2));
    assert_eq!(tree.get(b"abZZZ"), Some(&3));
}

#[test]
fn no_false_match_on_partial_prefix() {
    let mut tree = ARTMap::new();
    tree.put(b"abcdef", 1);
    assert!(tree.get(b"abcXXX").is_none());
    assert!(tree.get(b"abc").is_none());
    assert!(tree.get(b"abcdefg").is_none());
}

#[test]
fn four_children_in_node4() {
    let mut tree = ARTMap::new();
    for i in 0..4u8 {
        tree.put(&[b'a' + i], i as i32);
    }
    for i in 0..4u8 {
        assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
    }
    assert_eq!(tree.len(), 4);
}

#[test]
fn node4_to_node16() {
    let mut tree = ARTMap::new();
    for i in 0..5u8 {
        tree.put(&[b'a' + i], i as i32);
    }
    for i in 0..5u8 {
        assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
    }
    assert_eq!(tree.len(), 5);
}

#[test]
fn node16_to_node48() {
    let mut tree = ARTMap::new();
    for i in 0..17u8 {
        tree.put(&[b'a' + i], i as i32);
    }
    for i in 0..17u8 {
        assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
    }
    assert_eq!(tree.len(), 17);
}

#[test]
fn node48_to_node256() {
    let mut tree = ARTMap::new();
    for i in 0..49u8 {
        tree.put(&[b'A' + i], i as i32);
    }
    for i in 0..49u8 {
        assert_eq!(tree.get(&[b'A' + i]), Some(&(i as i32)));
    }
    assert_eq!(tree.len(), 49);
}

#[test]
fn full_byte_range() {
    let mut tree = ARTMap::new();
    for b in 0..=255u8 {
        tree.put(&[b], b as i32);
    }
    assert_eq!(tree.len(), 256);
    for b in 0..=255u8 {
        assert_eq!(tree.get(&[b]), Some(&(b as i32)));
    }
}

#[test]
fn stress_1000_sequential() {
    let mut tree = ARTMap::new();
    let keys: Vec<Vec<u8>> = (0..1000)
        .map(|i| format!("key{:04}", i).into_bytes())
        .collect();
    for (i, k) in keys.iter().enumerate() {
        tree.put(k, i);
    }
    assert_eq!(tree.len(), 1000);
    for (i, k) in keys.iter().enumerate() {
        assert_eq!(tree.get(k), Some(&i));
    }
}

#[test]
fn delete_single() {
    let mut tree = ARTMap::new();
    tree.put(b"k", 1);
    assert!(tree.delete(b"k"));
    assert!(tree.get(b"k").is_none());
    assert_eq!(tree.len(), 0);
}

#[test]
fn delete_missing() {
    let mut tree: ARTMap<i32> = ARTMap::new();
    assert!(!tree.delete(b"x"));
}

#[test]
fn delete_missing_after_real_delete() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.delete(b"a");
    assert!(!tree.delete(b"a"));
}

#[test]
fn delete_one_of_many() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    tree.put(b"c", 3);
    tree.delete(b"b");
    assert_eq!(tree.get(b"a"), Some(&1));
    assert!(tree.get(b"b").is_none());
    assert_eq!(tree.get(b"c"), Some(&3));
    assert_eq!(tree.len(), 2);
}

#[test]
fn delete_prefix_key_middle() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"ab", 2);
    tree.put(b"abc", 3);
    tree.delete(b"ab");
    assert_eq!(tree.get(b"a"), Some(&1));
    assert!(tree.get(b"ab").is_none());
    assert_eq!(tree.get(b"abc"), Some(&3));
    assert_eq!(tree.len(), 2);
}

#[test]
fn delete_prefix_key_shortest() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"ab", 2);
    tree.put(b"abc", 3);
    tree.delete(b"a");
    assert!(tree.get(b"a").is_none());
    assert_eq!(tree.get(b"ab"), Some(&2));
    assert_eq!(tree.get(b"abc"), Some(&3));
}

#[test]
fn delete_prefix_key_longest() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"ab", 2);
    tree.put(b"abc", 3);
    tree.delete(b"abc");
    assert_eq!(tree.get(b"a"), Some(&1));
    assert_eq!(tree.get(b"ab"), Some(&2));
    assert!(tree.get(b"abc").is_none());
}

#[test]
fn delete_returns_false_for_prefix_of_existing() {
    let mut tree = ARTMap::new();
    tree.put(b"abc", 1);
    assert!(!tree.delete(b"ab"));
    assert!(!tree.delete(b"a"));
    assert_eq!(tree.get(b"abc"), Some(&1));
}

#[test]
fn delete_returns_false_for_extension() {
    let mut tree = ARTMap::new();
    tree.put(b"ab", 1);
    assert!(!tree.delete(b"abc"));
    assert_eq!(tree.get(b"ab"), Some(&1));
}

#[test]
fn reinsert_after_delete() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.delete(b"a");
    tree.put(b"a", 2);
    assert_eq!(tree.get(b"a"), Some(&2));
    assert_eq!(tree.len(), 1);
}

#[test]
fn delete_all_then_reuse() {
    let mut tree = ARTMap::new();
    for i in 0..10u8 {
        tree.put(&[b'a' + i], i as i32);
    }
    for i in 0..10u8 {
        assert!(tree.delete(&[b'a' + i]));
    }
    assert_eq!(tree.len(), 0);
    tree.put(b"fresh", 1);
    assert_eq!(tree.get(b"fresh"), Some(&1));
}

#[test]
fn shrink_node16_to_node4() {
    let mut tree = ARTMap::new();
    for i in 0..5u8 {
        tree.put(&[b'a' + i], i as i32);
    }
    tree.delete(b"e");
    for i in 0..4u8 {
        assert_eq!(tree.get(&[b'a' + i]), Some(&(i as i32)));
    }
    assert_eq!(tree.len(), 4);
}

#[test]
fn shrink_to_single_leaf() {
    let mut tree = ARTMap::new();
    for i in 0..5u8 {
        tree.put(&[b'a' + i], i as i32);
    }
    for i in 1..5u8 {
        tree.delete(&[b'a' + i]);
    }
    assert_eq!(tree.get(b"a"), Some(&0));
    assert_eq!(tree.len(), 1);
}

#[test]
fn prefix_recompression_after_delete() {
    let mut tree = ARTMap::new();
    tree.put(b"abc", 1);
    tree.put(b"abd", 2);
    tree.delete(b"abd");
    assert_eq!(tree.get(b"abc"), Some(&1));
    tree.put(b"abc", 99);
    assert_eq!(tree.get(b"abc"), Some(&99));
}

#[test]
fn delete_all_200() {
    let mut tree = ARTMap::new();
    let keys: Vec<Vec<u8>> = (0..200).map(|i| format!("k{}", i).into_bytes()).collect();
    for k in &keys {
        tree.put(k, 0);
    }
    for k in &keys {
        assert!(tree.delete(k));
    }
    assert_eq!(tree.len(), 0);
    tree.put(b"fresh", 1);
    assert_eq!(tree.get(b"fresh"), Some(&1));
}

#[test]
fn items_empty() {
    let tree: ARTMap<i32> = ARTMap::new();
    assert!(tree.items().is_empty());
}

#[test]
fn items_sorted_order() {
    let mut tree = ARTMap::new();
    tree.put(b"c", 3);
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    let items: Vec<_> = tree.items().into_iter().map(|(k, &v)| (k, v)).collect();
    assert_eq!(
        items,
        vec![
            (b"a".as_slice(), 1),
            (b"b".as_slice(), 2),
            (b"c".as_slice(), 3)
        ]
    );
}

#[test]
fn items_with_prefix_keys() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"ab", 2);
    tree.put(b"abc", 3);
    let keys: Vec<_> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
    assert_eq!(keys, vec![b"a".to_vec(), b"ab".to_vec(), b"abc".to_vec()]);
}

#[test]
fn items_empty_key_first() {
    let mut tree = ARTMap::new();
    tree.put(b"", 0);
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    let keys: Vec<_> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
    assert_eq!(keys, vec![b"".to_vec(), b"a".to_vec(), b"b".to_vec()]);
}

#[test]
fn items_after_deletes() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    tree.put(b"c", 3);
    tree.delete(b"b");
    let keys: Vec<_> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
    assert_eq!(keys, vec![b"a".to_vec(), b"c".to_vec()]);
}

#[test]
fn items_after_growth() {
    let mut tree = ARTMap::new();
    let mut keys: Vec<Vec<u8>> = (0..49u8).map(|i| vec![b'A' + i]).collect();
    for (i, k) in keys.iter().enumerate() {
        tree.put(k, i as i32);
    }
    let result: Vec<Vec<u8>> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
    keys.sort();
    assert_eq!(result, keys);
}

#[test]
fn items_1000_sorted() {
    let mut tree = ARTMap::new();
    let mut keys: Vec<Vec<u8>> = (0..1000)
        .map(|i| format!("key{:04}", i).into_bytes())
        .collect();
    for (i, k) in keys.iter().enumerate() {
        tree.put(k, i);
    }
    let result: Vec<Vec<u8>> = tree.items().into_iter().map(|(k, _)| k.to_vec()).collect();
    keys.sort();
    assert_eq!(result, keys);
}

fn keys_from_range(tree: &ARTMap<i32>, from: Option<&[u8]>, to: Option<&[u8]>) -> Vec<Vec<u8>> {
    tree.range(from, to)
        .into_iter()
        .map(|(k, _)| k.to_vec())
        .collect()
}

#[test]
fn range_from_key() {
    let mut tree = ARTMap::new();
    for c in b"abcde" {
        tree.put(&[*c], *c as i32);
    }
    assert_eq!(
        keys_from_range(&tree, Some(b"c"), None),
        vec![b"c".to_vec(), b"d".to_vec(), b"e".to_vec()]
    );
}

#[test]
fn range_to_key() {
    let mut tree = ARTMap::new();
    for c in b"abcde" {
        tree.put(&[*c], *c as i32);
    }
    assert_eq!(
        keys_from_range(&tree, None, Some(b"c")),
        vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
    );
}

#[test]
fn range_from_and_to() {
    let mut tree = ARTMap::new();
    for c in b"abcde" {
        tree.put(&[*c], *c as i32);
    }
    assert_eq!(
        keys_from_range(&tree, Some(b"b"), Some(b"d")),
        vec![b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
    );
}

#[test]
fn range_empty_result() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"z", 26);
    assert!(keys_from_range(&tree, Some(b"m"), Some(b"n")).is_empty());
}

#[test]
fn range_from_beyond_all() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    assert!(keys_from_range(&tree, Some(b"z"), None).is_empty());
}

#[test]
fn range_to_before_all() {
    let mut tree = ARTMap::new();
    tree.put(b"m", 1);
    tree.put(b"n", 2);
    assert!(keys_from_range(&tree, None, Some(b"a")).is_empty());
}

#[test]
fn range_exact_bounds() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    tree.put(b"c", 3);
    let r = tree.range(Some(b"b"), Some(b"b"));
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].0, b"b");
}

#[test]
fn range_exact_bounds_missing() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"c", 3);
    assert!(tree.range(Some(b"b"), Some(b"b")).is_empty());
}

#[test]
fn range_with_shared_prefix() {
    let mut tree = ARTMap::new();
    tree.put(b"abc", 1);
    tree.put(b"abd", 2);
    tree.put(b"abe", 3);
    tree.put(b"abf", 4);
    let items = tree.range(Some(b"abd"), Some(b"abe"));
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].0, b"abd");
    assert_eq!(items[1].0, b"abe");
}

#[test]
fn range_prefix_keys() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"ab", 2);
    tree.put(b"abc", 3);
    tree.put(b"abd", 4);
    tree.put(b"b", 5);
    let items = tree.range(Some(b"ab"), Some(b"abd"));
    let keys: Vec<_> = items.into_iter().map(|(k, _)| k.to_vec()).collect();
    assert_eq!(keys, vec![b"ab".to_vec(), b"abc".to_vec(), b"abd".to_vec()]);
}

#[test]
fn range_from_is_prefix_of_keys() {
    let mut tree = ARTMap::new();
    tree.put(b"abc", 1);
    tree.put(b"abd", 2);
    tree.put(b"xyz", 3);
    let keys = keys_from_range(&tree, Some(b"ab"), None);
    assert_eq!(
        keys,
        vec![b"abc".to_vec(), b"abd".to_vec(), b"xyz".to_vec()]
    );
}

#[test]
fn range_to_is_prefix_of_keys() {
    let mut tree = ARTMap::new();
    tree.put(b"a", 1);
    tree.put(b"abc", 2);
    tree.put(b"abd", 3);
    tree.put(b"b", 4);
    let keys = keys_from_range(&tree, None, Some(b"ab"));
    assert_eq!(keys, vec![b"a".to_vec()]);
}

#[test]
fn range_with_empty_from_key() {
    let mut tree = ARTMap::new();
    tree.put(b"", 0);
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    let keys = keys_from_range(&tree, Some(b""), None);
    assert_eq!(keys, vec![b"".to_vec(), b"a".to_vec(), b"b".to_vec()]);
}

#[test]
fn range_with_empty_to_key() {
    let mut tree = ARTMap::new();
    tree.put(b"", 0);
    tree.put(b"a", 1);
    tree.put(b"b", 2);
    let keys = keys_from_range(&tree, None, Some(b""));
    assert_eq!(keys, vec![b"".to_vec()]);
}

#[test]
fn range_stress_500() {
    let mut tree = ARTMap::new();
    let keys: Vec<Vec<u8>> = (0..500)
        .map(|i| format!("k{:04}", i).into_bytes())
        .collect();
    for k in &keys {
        tree.put(k, 0);
    }
    for &(lo, hi) in &[(50, 100), (0, 10), (490, 499), (200, 200)] {
        let lo_key = format!("k{:04}", lo).into_bytes();
        let hi_key = format!("k{:04}", hi).into_bytes();
        let result = keys_from_range(&tree, Some(&lo_key), Some(&hi_key));
        let expected: Vec<Vec<u8>> = (lo..=hi)
            .map(|i| format!("k{:04}", i).into_bytes())
            .collect();
        assert_eq!(result, expected, "range [{}, {}]", lo, hi);
    }
}

#[test]
fn range_no_overlap() {
    let mut tree = ARTMap::new();
    tree.put(b"aaa", 1);
    tree.put(b"bbb", 2);
    tree.put(b"ccc", 3);
    assert!(tree.range(Some(b"d"), Some(b"z")).is_empty());
    assert!(tree.range(Some(b"0"), Some(b"1")).is_empty());
}

#[test]
fn range_deep_tree() {
    let base = "a".repeat(50);
    let mut tree = ARTMap::new();
    let keys: Vec<Vec<u8>> = (0..10)
        .map(|i| format!("{}{}", base, (b'a' + i) as char).into_bytes())
        .collect();
    for k in &keys {
        tree.put(k, 0);
    }
    let result = keys_from_range(&tree, Some(&keys[3]), Some(&keys[7]));
    assert_eq!(result, keys[3..8].to_vec());
}

#[test]
fn interleaved_insert_delete() {
    use std::collections::HashMap;

    let mut tree = ARTMap::new();
    let mut live: HashMap<Vec<u8>, i32> = HashMap::new();
    let mut rng: u64 = 99;
    let mut next = || -> u64 {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        rng >> 33
    };
    for _ in 0..2000 {
        let k = format!("k{}", next() % 201).into_bytes();
        if next() % 10 < 7 {
            let v = (next() % 100000) as i32;
            tree.put(&k, v);
            live.insert(k, v);
        } else {
            let existed = live.remove(&k).is_some();
            assert_eq!(tree.delete(&k), existed);
        }
    }
    assert_eq!(tree.len(), live.len());
    for (k, v) in &live {
        assert_eq!(tree.get(k), Some(v));
    }
    let items = tree.items();
    let mut expected: Vec<_> = live.iter().map(|(k, v)| (k.clone(), *v)).collect();
    expected.sort();
    let actual: Vec<_> = items.into_iter().map(|(k, &v)| (k.to_vec(), v)).collect();
    assert_eq!(actual, expected);
}

#[test]
fn range_matches_full_scan() {
    let mut tree = ARTMap::new();
    let keys: Vec<Vec<u8>> = (0..200)
        .map(|i| format!("k{:04}", i).into_bytes())
        .collect();
    for k in &keys {
        tree.put(k, 0);
    }
    let lo = b"k0050".to_vec();
    let hi = b"k0150".to_vec();
    let range_result = keys_from_range(&tree, Some(&lo), Some(&hi));
    let full_result: Vec<Vec<u8>> = tree
        .items()
        .into_iter()
        .filter(|(k, _)| k >= &lo.as_slice() && k <= &hi.as_slice())
        .map(|(k, _)| k.to_vec())
        .collect();
    assert_eq!(range_result, full_result);
}
