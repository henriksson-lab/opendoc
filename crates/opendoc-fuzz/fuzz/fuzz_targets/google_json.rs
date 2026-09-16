#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    opendoc_fuzz::targets::google_json(data);
});
