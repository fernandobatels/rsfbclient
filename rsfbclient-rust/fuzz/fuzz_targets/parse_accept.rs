#![no_main]
use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use rsfbclient_rust::parse_accept;

fuzz_target!(|data: &[u8]| {
    parse_accept(&mut Bytes::copy_from_slice(data)).ok();
});
