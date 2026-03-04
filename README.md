# Verus Proptest

## Summary

- `#[verus_proptest::verus_proptest]` served as the main entry. It scans through `verus!` and generate `Testable` shim for every functions annotated with `#[verus_proptest::generate]` attribute.
- At runtime, `proptest` generates random test inputs.
- For each generated input, `databake` is used to encode generated value into rust code, which is then written to a temporary file as a test case.
- Verus verifies this temporary file. If verification succeeds, the test case passes. If verification fails, proptest attempts to shrink the input to find a minimal failing case.
- The process repeats until either a failing case is found or the configured number of test cases have been verified successfully.

## Example

```rust
use vstd::prelude::*;

#[verus_proptest::verus_proptest]
verus! {

#[verus_proptest::generate]
fn encode_shift(s: &Vec<u8>) -> (t: Vec<u8>)
    requires
        forall|i: int| #![trigger s[i]] 0 <= i < s.len() ==> 65 <= s[i] <= 90,
    ensures
        s.len() == t.len(),
        forall|i: int| #![auto] 0 <= i < t.len() ==> t[i] == encode_char_spec(s[i] as int),
{
    // implementation
}

} // verus!
```

<details>
  <summary>Generated <code>Testable</code> shim</summary>

  ```rust
  struct EncodeShift;
  impl ::verus_proptest::Testable for EncodeShift {
    type Args = (Vec<u8>,);
    type Ret = Vec<u8>;
    const ARGS: ::verus_proptest::Args = ::verus_proptest::Args(
      &[
        ::verus_proptest::Arg {
            pattern: "s",
            ref_stack: ::verus_proptest::RefStack(&[::verus_proptest::Ref::Ref]),
        },
      ],
    );
    const RET: Option<&str> = Some("t");
    const REQUIRES: Option<&str> = Some(
      "assert(forall | i : int | # ! [trigger s [i]] 0 <= i < s.len() ==> 65 <= s [i]\n<= 90);",
    );
    const ENSURES: Option<&str> = Some(
      "assert(s.len() == t.len());\nassert(forall | i : int | # ! [auto] 0 <= i < t.len() ==> t [i] ==\nencode_char_spec(s [i] as int));\nassert(forall | i : int | # ! [auto] 0 <= i < t.len() ==>\ndecode_char_spec(t [i] as int) == s [i]);",
    );
    fn run(args: Self::Args) -> Self::Ret {
      encode_shift(&args.0)
    }
  }
  ```
    
</details>

Then create a test function that invokes the generated test:

```rust
#[test]
fn test_encode_shift() {
    verus_proptest::test::<EncodeShift>().unwrap();
}
```

## TODO

- Requires & ensures cannot reference environment, except bindings of the function. Need to copy entire crate to the temporary file.
- Better strategy synthesis. Currently all values are generated randomly, but we could use the preconditions to guide generation towards more interesting cases.
  - Dependency
  - Range
  - Vec & other container pattern recognition
- Programically invoke verus verifier instead of writing temporary files.
- Parallelize test execution (not sure if it's possible).
