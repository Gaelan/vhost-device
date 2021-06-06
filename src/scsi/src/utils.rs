// Alias to assert - used for cases where I'm only handling one case for now,
// but there are almost certainly other cases I should be handling - this way,
// these are greppable separately from actual "runtime invariant that should
// never happen" assertions
#[macro_export]
macro_rules! hope {
    ($cond:expr) => {
        assert!($cond, "hope is fleeting");
    };
}
