//! Arc4 stream cipher implementation for the firebird wire encryption (Wire Protocol 13)

use std::io::{Read, Write};

#[derive(Clone)]
pub struct Arc4 {
    i: u8,
    j: u8,
    state: [u8; 256],
}

impl Arc4 {
    pub fn new(key: &[u8]) -> Arc4 {
        assert!(!key.is_empty() && key.len() <= 256);
        let mut rc4 = Arc4 {
            i: 0,
            j: 0,
            state: [0; 256],
        };
        for (i, x) in rc4.state.iter_mut().enumerate() {
            *x = i as u8;
        }
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j
                .wrapping_add(rc4.state[i])
                .wrapping_add(key[i % key.len()]);
            rc4.state.swap(i, j as usize);
        }
        rc4
    }

    fn next(&mut self) -> u8 {
        self.i = self.i.wrapping_add(1);
        self.j = self.j.wrapping_add(self.state[self.i as usize]);
        self.state.swap(self.i as usize, self.j as usize);

        self.state[(self.state[self.i as usize].wrapping_add(self.state[self.j as usize])) as usize]
    }

    fn process(&mut self, input: &[u8], output: &mut [u8]) {
        assert!(input.len() == output.len());
        for (x, y) in input.iter().zip(output.iter_mut()) {
            *y = *x ^ self.next();
        }
    }
}

/// Wraps a stream, encoding / decoding the data
pub struct Arc4Stream<S> {
    read_rc4: Box<Arc4>,
    write_rc4: Box<Arc4>,
    enc_buf: Box<[u8]>,
    stream: S,
}

impl<S> Arc4Stream<S> {
    pub fn new(stream: S, key: &[u8], buf_len: usize) -> Self {
        Self {
            read_rc4: Box::new(Arc4::new(key)),
            write_rc4: Box::new(Arc4::new(key)),
            enc_buf: vec![0; buf_len].into_boxed_slice(),
            stream,
        }
    }
}

impl<S: Read> Read for Arc4Stream<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Self {
            read_rc4,
            enc_buf,
            stream,
            ..
        } = self;

        let max_len = buf.len().min(enc_buf.len());
        // Read to the encrypted buffer
        let len = stream.read(&mut enc_buf[..max_len])?;
        // Decrypt
        read_rc4.process(&enc_buf[..len], &mut buf[..len]);

        Ok(len)
    }
}

impl<S: Write> Write for Arc4Stream<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let Self {
            write_rc4,
            enc_buf,
            stream,
            ..
        } = self;

        let max_len = buf.len().min(enc_buf.len());
        // Encrypt
        write_rc4.process(&buf[..max_len], &mut enc_buf[..max_len]);
        // Write encrypted data
        let len = stream.write(&enc_buf[..max_len])?;

        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

#[test]
fn arc4_test() {
    let mut a1 = Arc4::new(b"a key");

    let enc = &mut [0; 10];

    a1.process(b"plain text", enc);
    assert_eq!(enc, b"\x4b\x4b\xdc\x65\x02\xb3\x08\x17\x48\x82");

    let mut a2 = Arc4::new(b"a key");

    let plain = &mut [0; 10];

    a2.process(enc, plain);
    assert_eq!(plain, b"plain text");
}
