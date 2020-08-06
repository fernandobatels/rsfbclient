//! SRP client implementation.
//!
//! # Usage
//! First create SRP client struct by passing to it SRP parameters (shared
//! between client and server) and randomly generated `a`:
//!
//! ```ignore
//! use srp::groups::G_2048;
//! use sha2::Sha256;
//!
//! let mut a = [0u8; 64];
//! rng.fill_bytes(&mut a);
//! let client = SrpClient::<Sha256>::new(&a, &G_2048);
//! ```
//!
//! Next send handshake data (username and `a_pub`) to the server and receive
//! `salt` and `b_pub`:
//!
//! ```ignore
//! let a_pub = client.get_a_pub();
//! let (salt, b_pub) = conn.send_handshake(username, a_pub);
//! ```
//!
//! Compute private key using `salt` with any password hashing function.
//! You can use method from SRP-6a, but it's recommended to use specialized
//! password hashing algorithm instead (e.g. PBKDF2, argon2 or scrypt).
//! Next create verifier instance, note that `get_verifier` consumes client and
//! can return error in case of malicious `b_pub`.
//!
//! ```ignore
//! let private_key = srp_private_key::<Sha256>(username, password, salt);
//! let verifier = client.get_verifier(&private_key, &b_pub)?;
//! ```
//!
//! Finally verify the server: first generate user proof,
//! send it to the server and verify server proof in the reply. Note that
//! `verify_server` method will return error in case of incorrect server reply.
//!
//! ```ignore
//! let user_proof = verifier.get_proof();
//! let server_proof = conn.send_proof(user_proof);
//! let key = verifier.verify_server(server_proof)?;
//! ```
//!
//! `key` contains shared secret key between user and the server. Alternatively
//! you can directly extract shared secret key using `get_key()` method and
//! handle authentication through different (secure!) means (e.g. by using
//! authenticated cipher mode).
//!
//! For user registration on the server first generate salt (e.g. 32 bytes long)
//! and get password verifier which depends on private key. Send useranme, salt
//! and password verifier over protected channel to protect against
//! Man-in-the-middle (MITM) attack for registration.
//!
//! ```ignore
//! let pwd_verifier = client.get_password_verifier(&private_key);
//! conn.send_registration_data(username, salt, pwd_verifier);
//! ```

#![allow(dead_code, clippy::many_single_char_names)]

use std::{error, fmt, marker::PhantomData};

use digest::Digest;
use generic_array::GenericArray;
use num_bigint::BigUint;

/// SRP client state before handshake with the server.
pub struct SrpClient<'a, D: Digest> {
    params: &'a SrpGroup,

    a: BigUint,
    a_pub: BigUint,

    d: PhantomData<D>,
}

/// SRP client state after handshake with the server.
pub struct SrpClientVerifier<D: Digest> {
    proof: GenericArray<u8, D::OutputSize>,
    key: GenericArray<u8, D::OutputSize>,
}

/// Compute user private key as described in the RFC 5054. Consider using proper
/// password hashing algorithm instead.
pub fn srp_private_key<D: Digest>(
    username: &[u8],
    password: &[u8],
    salt: &[u8],
) -> GenericArray<u8, D::OutputSize> {
    let p = D::new()
        .chain(username)
        .chain(b":")
        .chain(password)
        .finalize();

    D::new().chain(salt).chain(&p).finalize()
}

impl<'a, D: Digest> SrpClient<'a, D> {
    /// Create new SRP client instance.
    pub fn new(a: &[u8], params: &'a SrpGroup) -> Self {
        let a = BigUint::from_bytes_be(a);
        let a_pub = params.powm(&a);

        Self {
            params,
            a,
            a_pub,
            d: Default::default(),
        }
    }

    /// Get password verfier for user registration on the server
    pub fn get_password_verifier(&self, private_key: &[u8]) -> Vec<u8> {
        let x = BigUint::from_bytes_be(private_key);
        let v = self.params.powm(&x);
        v.to_bytes_be()
    }

    fn calc_key(
        &self,
        b_pub: &BigUint,
        x: &BigUint,
        u: &BigUint,
    ) -> GenericArray<u8, D::OutputSize> {
        let n = &self.params.n;
        let k = self.params.compute_k::<D>();
        let interm = (k * self.params.powm(x)) % n;
        // Because we do operation in modulo N we can get: (kv + g^b) < kv
        let v = if *b_pub > interm {
            (b_pub - &interm) % n
        } else {
            (n + b_pub - &interm) % n
        };
        // S = |B - kg^x| ^ (a + ux)
        let s = powm(&v, &(&self.a + (u * x) % n), n);
        D::digest(&s.to_bytes_be())
    }

    /// Process server reply to the handshake.
    pub fn process_reply(
        self,
        user: &[u8],
        salt: &[u8],
        private_key: &[u8],
        b_pub: &[u8],
    ) -> Result<SrpClientVerifier<D>, SrpAuthError> {
        let u = {
            BigUint::from_bytes_be(
                &D::new()
                    .chain(&self.a_pub.to_bytes_be())
                    .chain(b_pub)
                    .finalize(),
            )
        };

        let b_pub = BigUint::from_bytes_be(b_pub);

        // Safeguard against malicious B
        if &b_pub % &self.params.n == BigUint::default() {
            return Err(SrpAuthError {
                description: "Malicious b_pub value",
            });
        }

        let x = BigUint::from_bytes_be(private_key);
        let key = self.calc_key(&b_pub, &x, &u);
        // M = H(pow(H(N), H(g)) % N | H(U) | s | A | B | K)
        let proof = {
            let hn = {
                let n = &self.params.n;

                BigUint::from_bytes_be(&D::new().chain(n.to_bytes_be()).finalize())
            };
            let hg = {
                let g = &self.params.g;

                BigUint::from_bytes_be(&D::new().chain(g.to_bytes_be()).finalize())
            };
            let hu = D::new().chain(user).finalize();

            D::new()
                .chain((hn.modpow(&hg, &self.params.n)).to_bytes_be())
                .chain(hu)
                .chain(salt)
                .chain(&self.a_pub.to_bytes_be())
                .chain(&b_pub.to_bytes_be())
                .chain(&key)
                .finalize()
        };

        Ok(SrpClientVerifier { proof, key })
    }

    /// Get public ephemeral value for handshaking with the server.
    pub fn get_a_pub(&self) -> Vec<u8> {
        self.a_pub.to_bytes_be()
    }
}

impl<D: Digest> SrpClientVerifier<D> {
    /// Get shared secret key without authenticating server, e.g. for using with
    /// authenticated encryption modes. DO NOT USE this method without
    /// some kind of secure authentication
    pub fn get_key(self) -> GenericArray<u8, D::OutputSize> {
        self.key
    }

    /// Verification data for sending to the server.
    pub fn get_proof(&self) -> GenericArray<u8, D::OutputSize> {
        self.proof.clone()
    }
}

pub fn powm(base: &BigUint, exp: &BigUint, modulus: &BigUint) -> BigUint {
    let zero = BigUint::from(0u32);
    let one = BigUint::from(1u32);
    let two = BigUint::from(2u32);
    let mut exp = exp.clone();
    let mut result = one.clone();
    let mut base = base % modulus;

    while exp > zero {
        if &exp % &two == one {
            result = (result * &base) % modulus;
        }
        exp >>= 1;
        base = (&base * &base) % modulus;
    }
    result
}

/// SRP authentication error.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct SrpAuthError {
    pub(crate) description: &'static str,
}

impl fmt::Display for SrpAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SRP authentication error")
    }
}

impl error::Error for SrpAuthError {
    fn description(&self) -> &str {
        self.description
    }
}

/// Group used for SRP computations
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SrpGroup {
    /// A large safe prime (N = 2q+1, where q is prime)
    pub n: BigUint,
    /// A generator modulo N
    pub g: BigUint,
}

impl SrpGroup {
    pub(crate) fn powm(&self, v: &BigUint) -> BigUint {
        powm(&self.g, v, &self.n)
    }

    /// Compute `k` with given hash function and return SRP parameters
    pub(crate) fn compute_k<D: Digest>(&self) -> BigUint {
        let n = self.n.to_bytes_be();
        let g_bytes = self.g.to_bytes_be();
        let mut buf = vec![0u8; n.len()];
        let l = n.len() - g_bytes.len();
        buf[l..].copy_from_slice(&g_bytes);

        BigUint::from_bytes_be(&D::new().chain(&n).chain(&buf).finalize())
    }
}
