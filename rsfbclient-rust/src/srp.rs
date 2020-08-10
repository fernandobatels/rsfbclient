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
use lazy_static::lazy_static;
use num_bigint::BigUint;

lazy_static! {
    /// Srp Group used by the firebird server
    pub static ref SRP_GROUP: SrpGroup = SrpGroup {
        n: BigUint::from_bytes_be(&[
            230, 125, 46, 153, 75, 47, 144, 12, 63, 65, 240, 143, 91, 178, 98, 126, 208, 212, 158,
            225, 254, 118, 122, 82, 239, 205, 86, 92, 214, 231, 104, 129, 44, 62, 30, 156, 232,
            240, 168, 190, 166, 203, 19, 205, 41, 221, 235, 247, 169, 109, 74, 147, 181, 93, 72,
            141, 240, 153, 161, 92, 137, 220, 176, 100, 7, 56, 235, 44, 189, 217, 168, 247, 186,
            181, 97, 171, 27, 13, 193, 198, 205, 171, 243, 3, 38, 74, 8, 209, 188, 169, 50, 209,
            241, 238, 66, 139, 97, 157, 151, 15, 52, 42, 186, 154, 101, 121, 59, 139, 47, 4, 26,
            229, 54, 67, 80, 193, 111, 115, 95, 86, 236, 188, 168, 123, 213, 123, 41, 231,
        ]),
        g: BigUint::from_bytes_be(&[2]),
    };
}

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

#[cfg(test)]
mod test {
    use super::SRP_GROUP;
    use crate::srp::{srp_private_key, SrpClient};
    use num_bigint::BigUint;
    use sha1::Sha1;

    #[test]
    fn srp_group_k_test() {
        use sha1::Digest;

        let k = {
            let n = SRP_GROUP.n.to_bytes_be();
            let g_bytes = SRP_GROUP.g.to_bytes_be();
            let mut buf = vec![0u8; n.len()];
            let l = n.len() - g_bytes.len();
            buf[l..].copy_from_slice(&g_bytes);

            BigUint::from_bytes_be(&sha1::Sha1::new().chain(&n).chain(&buf).finalize())
        };

        assert_eq!(
            "1277432915985975349439481660349303019122249719989",
            &k.to_string()
        );
    }

    #[test]
    fn srp_vals_test() {
        let user = b"sysdba";
        let password = b"masterkey";

        // Real one randomly generated
        let seed = [
            104, 168, 26, 157, 227, 194, 41, 70, 204, 234, 48, 50, 217, 147, 39, 186, 223, 61, 125,
            154, 223, 9, 54, 220, 163, 109, 222, 183, 78, 242, 217, 218,
        ];

        let cli = SrpClient::<Sha1>::new(&seed, &SRP_GROUP);

        assert_eq!(
            cli.get_a_pub(),
            BigUint::parse_bytes(
                b"140881421499567234926370707691929201584335514055692587180102084646282810733160001237892692305806957785292091467614922078328787082920091583399296847456481914076730273969778307678896596634071762017513173403243965936903761580099023780256639030075360658492420403842461445358536578442895018174380364815053686107255"
                , 10
            ).unwrap().to_bytes_be()
        );

        // Real ones are received from server
        let salt = b"9\xe0\xee\x06\xa9]\xbe\xa7\xe4V\x08\xb1g\xa1\x93\x19\xf6\x11\xcb@\t\xeb\x9c\xf8\xe5K_;\xd1\xeb\x0f\xde";
        let serv_pub = BigUint::parse_bytes(
            b"9664511961170061978805668776377548609867359536792555459451373100540811860853826881772164535593386333263225393199902079347793807335504376938377762257920751005873533468177562614066508611409115917792525726727162676806787115902775303095022305576987173568527110065130456437265884455358297687922316181717357090556", 
            10
        ).unwrap().to_bytes_be();

        let cli_priv = srp_private_key::<Sha1>(user, password, salt);

        assert_eq!(
            b"\xe7\xd1>*\xaag\x9a\xa9\"w\x17&>\xca\xff\x86+ '\xdc",
            &cli_priv[..]
        );

        let verifier = cli.process_reply(user, salt, &cli_priv, &serv_pub).unwrap();

        assert_eq!(
            b"C~\xe6\xad\xe1\x97d\xed\xbf\x16D7\xb1C\xbf\xb1\xc9\x92\xc4@",
            &verifier.get_proof()[..]
        );

        assert_eq!(
            b"\xd5,\xe6(\xf6\x04\xec\xdb\xf2\xa2J\xc8zw\xb0\x9a\x87O\xe8\xf7",
            &verifier.get_key()[..]
        );
    }
}
