use vstd::prelude::*;

#[verus_proptest::verus_proptest]
verus! {

#[verus_proptest::generate]
fn make_a_pile(n: usize) -> (pile: Vec<usize>)
    requires
        n + (2 * n) <= usize::MAX,
    ensures
        pile.len() == n,
        n > 0 ==> pile[0] == n,
        forall|i: int| #![trigger pile[i]] 1 <= i < n ==> pile[i] == pile[i - 1] + 2,
{
    if n == 0 {
        return vec![];
    }
    let mut pile = vec![n];
    for i in 1..n
        invariant
            pile.len() == i,
            pile[i - 1] + (2 * (n - i)) <= usize::MAX,
            forall|j: int| #![trigger pile[j]] 1 <= j < i ==> pile[j] == pile[j - 1] + 2,
            n > 0 ==> pile[0] == n,
    {
        let prev = pile[i - 1];
        pile.push(prev + 2);
    }
    pile
}

#[verus_proptest::generate]
fn make_a_pile_buggy(n: usize) -> (pile: Vec<usize>)
    requires
        n + (2 * n) <= usize::MAX,
    ensures
        pile.len() == n,
        n > 0 ==> pile[0] == n,
        forall|i: int| #![trigger pile[i]] 1 <= i < n ==> pile[i] == pile[i - 1] + 2,
{
    if n == 0 {
        return vec![];
    }
    let mut pile = vec![n];
    for i in 1..n
        invariant
            pile.len() == i,
            pile[i - 1] + (2 * (n - i)) <= usize::MAX,
            forall|j: int| #![trigger pile[j]] 1 <= j < i ==> pile[j] == pile[j - 1] + 2,
            n > 0 ==> pile[0] == n,
    {
        let prev = pile[i - 1];
        // BUG: Adding 3 instead of 2!
        pile.push(prev + 3);
    }
    pile
}

#[verus_proptest::generate]
fn make_a_pile_wrong_start(n: usize) -> (pile: Vec<usize>)
    requires
        n + (2 * n) <= usize::MAX,
    ensures
        pile.len() == n,
        n > 0 ==> pile[0] == n,
        forall|i: int| #![trigger pile[i]] 1 <= i < n ==> pile[i] == pile[i - 1] + 2,
{
    if n == 0 {
        return vec![];
    }
    // BUG: Start with n+1 instead of n

    let mut pile = vec![n + 1];
    for i in 1..n
        invariant
            pile.len() == i,
            pile[i - 1] + (2 * (n - i)) <= usize::MAX,
            forall|j: int| #![trigger pile[j]] 1 <= j < i ==> pile[j] == pile[j - 1] + 2,
    {
        let prev = pile[i - 1];
        pile.push(prev + 2);
    }
    pile
}

#[verus_proptest::generate]
fn make_a_pile_wrong_length(n: usize) -> (pile: Vec<usize>)
    requires
        n + (2 * n) <= usize::MAX,
    ensures
        pile.len() == n,
        n > 0 ==> pile[0] == n,
        forall|i: int| #![trigger pile[i]] 1 <= i < n ==> pile[i] == pile[i - 1] + 2,
{
    if n == 0 {
        return vec![];
    }
    let mut pile = vec![n];
    // BUG: Only iterate to n-1, producing a vector that's too short
    for i in 1..n.saturating_sub(1)
        invariant
            pile.len() == i,
            pile[i - 1] + (2 * (n - i)) <= usize::MAX,
            forall|j: int| #![trigger pile[j]] 1 <= j < i ==> pile[j] == pile[j - 1] + 2,
            n > 0 ==> pile[0] == n,
    {
        let prev = pile[i - 1];
        pile.push(prev + 2);
    }
    pile
}

} // verus!
#[test]
fn test_make_a_pile() {
    verus_proptest::test::<MakeAPile>().unwrap()
}

#[test]
fn test_make_a_pile_zero() {
    // Specific test for n=0 edge case
    let result = make_a_pile(0);
    assert_eq!(result.len(), 0);
}

#[test]
fn test_make_a_pile_buggy() {
    // This should fail and demonstrate shrinking
    // The bug is that we add 3 instead of 2
    let result = verus_proptest::test::<MakeAPileBuggy>();
    assert!(result.is_err(), "Expected the buggy version to fail");

    // Print the error to see the shrunk value
    if let Err(e) = result {
        eprintln!(
            "Buggy version (wrong increment) failed as expected with error: {:?}",
            e
        );
    }
}

#[test]
fn test_make_a_pile_wrong_start() {
    // This should fail because the first element is n+1 instead of n
    let result = verus_proptest::test::<MakeAPileWrongStart>();
    assert!(result.is_err(), "Expected wrong start version to fail");

    if let Err(e) = result {
        eprintln!("Wrong start version failed as expected with error: {:?}", e);
    }
}

#[test]
fn test_make_a_pile_wrong_length() {
    // This should fail because the vector is too short
    let result = verus_proptest::test::<MakeAPileWrongLength>();
    assert!(result.is_err(), "Expected wrong length version to fail");

    if let Err(e) = result {
        eprintln!(
            "Wrong length version failed as expected with error: {:?}",
            e
        );
    }
}
