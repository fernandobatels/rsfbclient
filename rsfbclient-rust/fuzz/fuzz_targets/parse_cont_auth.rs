#![no_main]
use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use rsfbclient_rust::parse_cont_auth;

fuzz_target!(|data: &[u8]| {
    parse_cont_auth(&mut Bytes::copy_from_slice(data)).ok();
});
