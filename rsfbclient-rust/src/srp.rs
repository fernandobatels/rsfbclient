//! SRP client implementation for the firebird authentication (Wire Protocol 13)

#![allow(clippy::many_single_char_names)]

use std::{error, fmt, marker::PhantomData};

use digest::Digest;
use generic_array::GenericArray;
use lazy_static::lazy_static;
use num_bigint::BigUint;
use sha1::Sha1;

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
    // Firebird hashes this with SHA1 for some reason
    key: GenericArray<u8, <Sha1 as Digest>::OutputSize>,
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

    // Firebird hashes this with SHA1 for some reason
    fn calc_key(
        &self,
        b_pub: &BigUint,
        x: &BigUint,
        u: &BigUint,
    ) -> GenericArray<u8, <Sha1 as Digest>::OutputSize> {
        let n = &self.params.n;
        let k = self.params.compute_k::<Sha1>();
        let interm = (k * self.params.powm(x)) % n;
        // Because we do operation in modulo N we can get: (kv + g^b) < kv
        let v = if *b_pub > interm {
            (b_pub - &interm) % n
        } else {
            (n + b_pub - &interm) % n
        };
        // S = |B - kg^x| ^ (a + ux)
        let s = powm(&v, &(&self.a + (u * x) % n), n);
        Sha1::digest(&s.to_bytes_be())
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
                // Firebird hashes this with SHA1 for some reason
                &Sha1::new()
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

                // Firebird hashes this with SHA1 for some reason
                BigUint::from_bytes_be(&Sha1::new().chain(n.to_bytes_be()).finalize())
            };
            let hg = {
                let g = &self.params.g;

                // Firebird hashes this with SHA1 for some reason
                BigUint::from_bytes_be(&Sha1::new().chain(g.to_bytes_be()).finalize())
            };
            // Firebird hashes this with SHA1 for some reason
            let hu = Sha1::new().chain(user).finalize();

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
    pub fn get_key(self) -> GenericArray<u8, <Sha1 as Digest>::OutputSize> {
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
    use sha2::Sha256;

    #[test]
    fn srp_group_k() {
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
    fn srp() {
        let user = b"sysdba";
        let password = b"masterkey";

        // Real one randomly generated
        let seed = b"h\xa8\x1a\x9d\xe3\xc2)F\xcc\xea02\xd9\x93'\xba\xdf=}\x9a\xdf\t6\xdc\xa3m\xde\xb7N\xf2\xd9\xda";

        let cli = SrpClient::<Sha1>::new(seed, &SRP_GROUP);

        assert_eq!(
            cli.get_a_pub(),
            BigUint::parse_bytes(
                b"c89f2e8556d724baee8781483c1397fa7b034afcdcb35b835c0caf54d3975980d5783cf8d81f0fb4f5bda079634ab78b9d6db31b4fa8ff961b04aba693fc867a9861fba9dcf306eae7b27b66c347c7ab0c87119168b68420cd1e211121533f90802f992d77485722dce0d19662414c0b21f09b750d439a16a4c9e9b076dcec77"
                , 16
            ).unwrap().to_bytes_be()
        );

        // Real ones are received from server
        let salt = b"9\xe0\xee\x06\xa9]\xbe\xa7\xe4V\x08\xb1g\xa1\x93\x19\xf6\x11\xcb@\t\xeb\x9c\xf8\xe5K_;\xd1\xeb\x0f\xde";
        let serv_pub = BigUint::parse_bytes(
            b"dc341bd8a8584dd0d69dda440550fb0f16c5b258f5b8fb422d5e2d92652006862cc6bb8dbd5fdd00f1744b75196a894dff7616742eb305ab1af96c39cbff4a80d088bf82c44e146cc176def524d700037608fd2c2bf193ffc59509d2cd3e1c792bfa9b623cbb3cf105b2ec0048f942f879253e0e3f26de88dd7a56e0a12d6fc", 
            16
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

    #[test]
    fn srp256() {
        let user = b"SYSDBA";
        let password = b"masterkey";

        // Real one randomly generated
        let seed = b"`\x97U'\x03\\\xf2\xad\x19\x89\x80o\x04\x07!\x0b\xc8\x1e\xdc\x04\xe2v*V\xaf\xd5)\xdd\xda-C\x93";

        let cli = SrpClient::<Sha256>::new(seed, &SRP_GROUP);

        // Real ones are received from server
        let salt = b"\x02\xe2h\x800\x00\x00\x00y\xa4x\xa7\x00\x00\x00\x02\xd1\xa6\x97\x90\x00\x00\x00&\xe1`\x1c\x00\x00\x00\x05O";
        let serv_pub = BigUint::parse_bytes(
            b"57bcd7d4241869e616ed54b5ab1814ca7b97b04bc269c4054a1325708a9f80821efeade02b875d2bda35c7e1e217ff7ef432c77720aa57baa250bdfbca47de56cccdfa8a6e82c74a99e4ae3db3f07f88d4b583169180fc78e70672e10746da0a27c5709e9b67fab4eaa7b426ac1cebf506d6cdaec1c1a0ade0e9e63a4a89d80a", 
            16,
        ).unwrap().to_bytes_be();

        let cli_priv = srp_private_key::<Sha1>(user, password, salt);

        assert_eq!(
            b"\xb9\xc1\xacv\x98\xb7\xbf\x90\xa5\xa2!\xb4S\xd6|\xad\x19\x91\x18\x07",
            &cli_priv[..]
        );

        let verifier = cli.process_reply(user, salt, &cli_priv, &serv_pub).unwrap();

        assert_eq!(
            b"Fu\xc1\x80V\xc0K\x00\xcc+\x99\x16b2L\"\xc6\xf0\x8b\xb9\x0b\xeb6wAk\x03F\x9aw\x03\x08",
            &verifier.get_proof()[..]
        );

        assert_eq!(
            b"\xe6*\x9c\xfd\xe3\xa3\xf8t[\xca\xa0\x06\x7f\xfc\x85z\xe6(\x84\xed",
            &verifier.get_key()[..]
        );
    }
}
