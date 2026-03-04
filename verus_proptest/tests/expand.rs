use std::{fs::File, io::Write};

use verus_proptest::{EnsuresCodegen, RequiresCodegen, Testable};
use vstd::prelude::*;

#[verus_proptest::verus_proptest]
verus! {

spec fn encode_char_spec(c: int) -> int
    recommends
        65 <= c <= 90,
{
    (c - 65 + 5) % 26 + 65
}

#[verus_proptest::generate]
fn encode_char(c: u8) -> (r: u8)
    requires
        65 <= c <= 90,
    ensures
        r == encode_char_spec(c as int),
        65 <= r <= 90,
{
    (c - 65 + 5) % 26 + 65
}

} // verus!
#[test]
fn test_expand_requires() {
    let mut tempfile = File::create("/tmp/verus_proptest_test_expand_requires.rs").unwrap();
    let args = (70,);
    let content = RequiresCodegen::<EncodeChar>::new(&args)
        .codegen()
        .unwrap()
        .to_string();
    println!("{content}");
    tempfile.write_all(content.as_bytes()).unwrap();
}

#[test]
fn test_expand_ensures() {
    let mut tempfile = File::create("/tmp/verus_proptest_test_expand_ensures.rs").unwrap();
    let args = (70,);
    let reqs = RequiresCodegen::<EncodeChar>::new(&args);
    let ret = EncodeChar::run(args);
    let content = EnsuresCodegen::new(reqs, &ret)
        .codegen()
        .unwrap()
        .to_string();
    println!("{content}");
    tempfile.write_all(content.as_bytes()).unwrap();
}
