#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate ruzstd;
use ruzstd::fse::round_trip;

fuzz_target!(|data: &[u8]| {
    round_trip(data);
});