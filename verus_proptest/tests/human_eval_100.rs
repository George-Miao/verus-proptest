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

} // verus!
#[test]
fn test_make_a_pile() {
    verus_proptest::test::<MakeAPile>().unwrap()
}
