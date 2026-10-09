//! Tamper seal for the release executable.
//!
//! The release build carries an 80-byte region (`REGION_INIT`: a 16-byte magic + 64 zero bytes).
//! After compiling, `cv-seal seal` signs a SHA-256 of the executable with an Ed25519 key and writes
//! the signature into that region. At start-up the app checks the signature against the public
//! key compiled into it: a renamed, rebranded or patched copy no longer matches and refuses to run.
//!
//! What the digest leaves out, so legitimate later steps don't break the seal:
//! - the signature bytes themselves;
//! - the PE checksum and the certificate-table directory entry, and everything after the image's
//!   last section (Authenticode code signing changes / appends exactly those);
//! - the 3 bytes after each `__TAURI_BUNDLE_TYPE_VAR_` (Tauri's bundler writes the package type
//!   there after the build hooks ran).
//!
//! The magic and the marker prefix are kept masked in the code, so the real magic exists exactly
//! once in the executable (the region) and a strings search doesn't show them.

use ring::signature::{Ed25519KeyPair, UnparsedPublicKey, ED25519};
use std::hint::black_box;

pub const MAGIC_LEN: usize = 16;
pub const SIG_LEN: usize = 64;
pub const REGION_LEN: usize = MAGIC_LEN + SIG_LEN;
/// Domain separation: what is signed is `DOMAIN || sha256(executable minus the exclusions)`.
const DOMAIN: &[u8] = b"cv-seal-v1";

/// Masks bytes at compile time (`unmask` reverses it at run time behind `black_box`, so the
/// compiler can't fold the plain bytes back into the binary).
pub const fn mask<const N: usize>(plain: [u8; N]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = plain[i] ^ (0xA5 ^ (i as u8).wrapping_mul(29));
        i += 1;
    }
    out
}

pub fn unmask<const N: usize>(masked: [u8; N]) -> [u8; N] {
    let m = black_box(masked);
    let mut out = [0u8; N];
    for i in 0..N {
        out[i] = m[i] ^ (0xA5 ^ (i as u8).wrapping_mul(29));
    }
    out
}

const MAGIC_MASKED: [u8; MAGIC_LEN] = mask(*b"\x8fcv\x01seal\x02region\x7e");
const BUNDLE_MARK_MASKED: [u8; 24] = mask(*b"__TAURI_BUNDLE_TYPE_VAR_");

/// The region as compiled into the executable (unsealed). Only the `static` that holds it may use
/// this constant, so the plain magic appears once.
pub const REGION_INIT: [u8; REGION_LEN] = {
    let magic = mask(MAGIC_MASKED);
    let mut r = [0u8; REGION_LEN];
    let mut i = 0;
    while i < MAGIC_LEN {
        r[i] = magic[i];
        i += 1;
    }
    r
};

#[derive(Debug, PartialEq, Eq)]
pub enum SealError {
    /// No region in the file (not a Clairvoyance release build, or the region was removed).
    NoRegion,
    /// The magic appears more than once: the file can't be sealed unambiguously.
    MultipleRegions,
    /// The region lies outside the PE image (after the last section).
    RegionOutsideImage,
    /// The region is still all zeros: the file was never sealed.
    Unsealed,
    /// The signature doesn't match: the file was changed after sealing, or sealed with another key.
    BadSignature,
    BadKey,
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SealError::NoRegion => "no seal region in the file",
            SealError::MultipleRegions => "the seal magic appears more than once",
            SealError::RegionOutsideImage => "the seal region is outside the executable image",
            SealError::Unsealed => "the file isn't sealed",
            SealError::BadSignature => "the seal doesn't match the file",
            SealError::BadKey => "invalid key",
        })
    }
}

impl std::error::Error for SealError {}

/// Offset of the region (its magic) in `file`; exactly one is required.
pub fn find_region(file: &[u8]) -> Result<usize, SealError> {
    let magic = unmask(MAGIC_MASKED);
    let mut it = memchr::memmem::find_iter(file, &magic);
    let first = it.next().ok_or(SealError::NoRegion)?;
    if it.next().is_some() {
        return Err(SealError::MultipleRegions);
    }
    Ok(first)
}

fn u16_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

/// PE layout facts the digest needs: (image end, byte ranges to leave out).
fn pe_layout(file: &[u8]) -> Option<(usize, Vec<(usize, usize)>)> {
    if file.get(0..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(file, 0x3C)?;
    if file.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = u16_at(file, coff + 2)?;
    let opt_size = u16_at(file, coff + 16)?;
    let opt = coff + 20;
    let dirs = match u16_at(file, opt)? {
        0x20b => opt + 112,
        0x10b => opt + 96,
        _ => return None,
    };
    let mut end = u32_at(file, opt + 60)?; // SizeOfHeaders
    let table = opt + opt_size;
    for s in 0..sections {
        let h = table + s * 40;
        let size = u32_at(file, h + 16)?;
        let ptr = u32_at(file, h + 20)?;
        if size > 0 {
            end = end.max(ptr + size);
        }
    }
    // CheckSum, and the certificate table entry (data directory 4).
    Some((end.min(file.len()), vec![(opt + 64, 4), (dirs + 4 * 8, 8)]))
}

/// SHA-256 of the file without the excluded bytes (they count as zeros; the overlay after the
/// image is left out entirely).
pub fn digest(file: &[u8], region: usize) -> [u8; 32] {
    let (end, mut skip) = pe_layout(file).unwrap_or((file.len(), Vec::new()));
    skip.push((region + MAGIC_LEN, SIG_LEN));
    let mark = unmask(BUNDLE_MARK_MASKED);
    for at in memchr::memmem::find_iter(&file[..end], &mark) {
        skip.push((at + mark.len(), 3));
    }
    skip.sort_unstable();

    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let zeros = [0u8; SIG_LEN];
    let mut pos = 0;
    for (start, len) in skip {
        let start = start.clamp(pos, end);
        let stop = (start + len).min(end);
        ctx.update(&file[pos..start]);
        ctx.update(&zeros[..stop - start]);
        pos = stop;
    }
    ctx.update(&file[pos..end]);
    let mut out = [0u8; 32];
    out.copy_from_slice(ctx.finish().as_ref());
    out
}

fn message(file: &[u8], region: usize) -> Vec<u8> {
    let mut m = DOMAIN.to_vec();
    m.extend_from_slice(&digest(file, region));
    m
}

fn check_region(file: &[u8]) -> Result<usize, SealError> {
    let region = find_region(file)?;
    let end = pe_layout(file).map_or(file.len(), |(e, _)| e);
    if region + REGION_LEN > end {
        return Err(SealError::RegionOutsideImage);
    }
    Ok(region)
}

/// Writes the signature into the file's region. `seed` is the 32-byte Ed25519 private key.
pub fn seal(file: &mut [u8], seed: &[u8; 32]) -> Result<(), SealError> {
    let region = check_region(file)?;
    let key = Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| SealError::BadKey)?;
    let sig = key.sign(&message(file, region));
    file[region + MAGIC_LEN..region + REGION_LEN].copy_from_slice(sig.as_ref());
    Ok(())
}

pub fn verify(file: &[u8], public_key: &[u8; 32]) -> Result<(), SealError> {
    let region = check_region(file)?;
    let sig = &file[region + MAGIC_LEN..region + REGION_LEN];
    if sig.iter().all(|&b| b == 0) {
        return Err(SealError::Unsealed);
    }
    UnparsedPublicKey::new(&ED25519, public_key).verify(&message(file, region), sig).map_err(|_| SealError::BadSignature)
}

pub fn public_key(seed: &[u8; 32]) -> Result<[u8; 32], SealError> {
    let key = Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| SealError::BadKey)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(ring::signature::KeyPair::public_key(&key).as_ref());
    Ok(out)
}

pub fn new_seed() -> [u8; 32] {
    use ring::rand::SecureRandom;
    let mut seed = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut seed).expect("system random");
    seed
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn from_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.trim();
    if s.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [7; 32];

    /// A minimal PE32+ file: headers, two sections, the region inside the second, then an overlay.
    fn fake_pe() -> Vec<u8> {
        let mut f = vec![0u8; 0x3000];
        f[0..2].copy_from_slice(b"MZ");
        let pe = 0x80usize;
        f[0x3C..0x40].copy_from_slice(&(pe as u32).to_le_bytes());
        f[pe..pe + 4].copy_from_slice(b"PE\0\0");
        let coff = pe + 4;
        f[coff + 2..coff + 4].copy_from_slice(&2u16.to_le_bytes());
        f[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        let opt = coff + 20;
        f[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        f[opt + 60..opt + 64].copy_from_slice(&0x400u32.to_le_bytes());
        let table = opt + 240;
        for (s, (ptr, size)) in [(0x400u32, 0x1000u32), (0x1400, 0x1000)].into_iter().enumerate() {
            let h = table + s * 40;
            f[h + 16..h + 20].copy_from_slice(&size.to_le_bytes());
            f[h + 20..h + 24].copy_from_slice(&ptr.to_le_bytes());
        }
        for (i, b) in f[0x400..0x2400].iter_mut().enumerate() {
            *b = (i * 31 % 251) as u8 | 1; // code-ish, no zero runs, never the magic
        }
        f[0x1800..0x1800 + REGION_LEN].copy_from_slice(&REGION_INIT);
        let mark = unmask(BUNDLE_MARK_MASKED);
        f[0x2000..0x2000 + 24].copy_from_slice(&mark);
        f[0x2018..0x201B].copy_from_slice(b"UNK");
        f
    }

    #[test]
    fn region_init_holds_the_magic_once() {
        assert_eq!(&REGION_INIT[..MAGIC_LEN], &unmask(MAGIC_MASKED));
        assert!(REGION_INIT[MAGIC_LEN..].iter().all(|&b| b == 0));
        assert_eq!(find_region(&fake_pe()), Ok(0x1800));
    }

    #[test]
    fn seal_then_verify() {
        let mut f = fake_pe();
        assert_eq!(verify(&f, &public_key(&SEED).unwrap()), Err(SealError::Unsealed));
        seal(&mut f, &SEED).unwrap();
        assert_eq!(verify(&f, &public_key(&SEED).unwrap()), Ok(()));
        assert_eq!(verify(&f, &public_key(&[8; 32]).unwrap()), Err(SealError::BadSignature));
    }

    #[test]
    fn any_change_in_the_image_breaks_it() {
        let mut f = fake_pe();
        seal(&mut f, &SEED).unwrap();
        let pk = public_key(&SEED).unwrap();
        for at in [0x02, 0x500, 0x17FF, 0x1800, 0x1800 + REGION_LEN + 1, 0x2017, 0x23FF] {
            let mut g = f.clone();
            g[at] ^= 0x40;
            assert!(verify(&g, &pk).is_err(), "change at {at:#x} not caught");
        }
    }

    #[test]
    fn code_signing_and_bundle_type_keep_it_valid() {
        let mut f = fake_pe();
        seal(&mut f, &SEED).unwrap();
        let pk = public_key(&SEED).unwrap();
        // Tauri's bundler writes the package type.
        f[0x2018..0x201B].copy_from_slice(b"NSS");
        // Authenticode: checksum, certificate table entry, padding + table appended.
        let opt = 0x80 + 24;
        f[opt + 64..opt + 68].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        let dir = opt + 112 + 32;
        f[dir..dir + 4].copy_from_slice(&0x3000u32.to_le_bytes());
        f[dir + 4..dir + 8].copy_from_slice(&0x200u32.to_le_bytes());
        f.extend_from_slice(&[0x5a; 0x205]);
        assert_eq!(verify(&f, &pk), Ok(()));
    }

    #[test]
    fn duplicate_or_missing_magic_is_refused() {
        let mut f = fake_pe();
        assert_eq!(find_region(&f[..0x1000]), Err(SealError::NoRegion));
        f[0x1C00..0x1C00 + MAGIC_LEN].copy_from_slice(&REGION_INIT[..MAGIC_LEN]);
        assert_eq!(seal(&mut f, &SEED), Err(SealError::MultipleRegions));
    }

    #[test]
    fn hex_round_trip() {
        let k = public_key(&SEED).unwrap();
        assert_eq!(from_hex::<32>(&to_hex(&k)), Some(k));
        assert_eq!(from_hex::<32>("zz"), None);
    }
}
