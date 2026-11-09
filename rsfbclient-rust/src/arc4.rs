//! Arc4 stream cipher implementation for the firebird wire encryption (Wire Protocol 13)

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
};

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

impl Arc4Stream<TcpStream> {
    /// Address of the server the wrapped stream is connected to
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.stream.peer_addr()
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
        // The underlying writer may accept a prefix, return zero, or fail.
        // Advance the real cipher only for plaintext actually consumed.
        let mut next = write_rc4.clone();
        next.process(&buf[..max_len], &mut enc_buf[..max_len]);
        let len = stream.write(&enc_buf[..max_len])?;
        if len == max_len {
            *write_rc4 = next;
        } else {
            write_rc4.process(&buf[..len], &mut enc_buf[..len]);
        }
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

#[test]
fn short_writes_and_interrupted_writes_preserve_the_cipher_position() {
    struct ShortWriter {
        data: Vec<u8>,
        calls: usize,
    }
    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.calls % 3 == 1 {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let count = bytes.len().min(2);
            self.data.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let key = b"transport test key";
    for buffer in [3, 16] {
        let mut stream = Arc4Stream::new(
            ShortWriter {
                data: Vec::new(),
                calls: 0,
            },
            key,
            buffer,
        );
        stream.write_all(b"first packet").unwrap();
        stream.write_all(b"second packet").unwrap();
        let encrypted = stream.stream.data;
        let mut decoded = vec![0; encrypted.len()];
        Arc4::new(key).process(&encrypted, &mut decoded);
        assert_eq!(decoded, b"first packetsecond packet");
    }
}

#[test]
fn failed_and_zero_writes_leave_unconsumed_cipher_bytes_available() {
    struct RetryWriter {
        data: Vec<u8>,
        attempt: usize,
    }
    impl Write for RetryWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.attempt += 1;
            match self.attempt {
                1 => Err(std::io::ErrorKind::WouldBlock.into()),
                2 => Ok(0),
                _ => {
                    self.data.extend_from_slice(bytes);
                    Ok(bytes.len())
                }
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let key = b"transport test key";
    let mut stream = Arc4Stream::new(
        RetryWriter {
            data: Vec::new(),
            attempt: 0,
        },
        key,
        16,
    );
    assert_eq!(
        stream.write(b"payload").unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(stream.write(b"payload").unwrap(), 0);
    stream.write_all(b"payload").unwrap();
    let mut decoded = vec![0; stream.stream.data.len()];
    Arc4::new(key).process(&stream.stream.data, &mut decoded);
    assert_eq!(decoded, b"payload");
}
