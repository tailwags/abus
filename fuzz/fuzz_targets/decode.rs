#![no_main]
use abus::Message;
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // fuzzed code goes here
    let mut bytes: BytesMut = data.into();

    let _ = Message::decode(&mut bytes);
});
