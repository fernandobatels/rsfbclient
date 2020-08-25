#![no_main]
use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use rsfbclient_rust::parse_xsqlda;

fuzz_target!(|data: &[u8]| {
    let mut xsqlda = Vec::new();
    parse_xsqlda(&mut Bytes::copy_from_slice(data), &mut xsqlda).ok();
});
