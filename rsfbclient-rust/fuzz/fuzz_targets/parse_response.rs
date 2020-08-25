#![no_main]
use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use rsfbclient_rust::parse_response;

fuzz_target!(|data: &[u8]| {
    parse_response(&mut Bytes::copy_from_slice(data)).ok();
});
