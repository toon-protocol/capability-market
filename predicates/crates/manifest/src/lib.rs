//! Canonical capability-market **input manifest** (toon-meta#121, resolving
//! the marketParamsHash fork capability-market#4 in favour of the manifest).
//!
//! Every mintable proposition declares its inputs as a content-addressed
//! manifest. The manifest is the market's frozen parameter set: its SHA-256 is
//! the `marketParamsHash` committed at `createMarket` and the
//! `market_params_hash` field of every [`journal::Journal`] the predicate
//! commits. **`marketParamsHash = sha256(canonical manifest bytes)`** — NOT
//! `sha256(raw params)`.
//!
//! # Why a dedicated encoding
//!
//! The same bytes are hashed off-chain (by proposition-authoring tooling, to
//! compute the value it pins in `createMarket`) and re-hashed **inside the
//! zkVM guest** (which reads the manifest, extracts its parameters by name,
//! and commits `sha256(manifest_bytes)` into the journal). Those two hashes
//! MUST agree byte-for-byte or the on-chain field check fails. A textual
//! format (JSON/YAML) would force a parser and a canonicalizer into the guest;
//! instead the manifest is a small **length-prefixed binary TLV** that the
//! guest parses with a handful of slice reads and no dependency beyond the
//! `sha2` it already links for the journal.
//!
//! # Canonical encoding (`manifest-v1`)
//!
//! All multi-byte integers are **little-endian**. The stream is:
//!
//! ```text
//! offset  size            field
//! 0       4               magic = b"TMF1"   (0x54 0x4D 0x46 0x31)
//! 4       2  (u16 LE)     entry_count       (>= 1)
//! 6       ...             entry_count entries, concatenated
//! ```
//!
//! Each entry is:
//!
//! ```text
//! 1  (u8)      kind        0x01 = HASH, 0x02 = VALUE, 0x03 = SLOT
//! 2  (u16 LE)  name_len    (>= 1)
//! N            name        name_len bytes, UTF-8
//! 4  (u32 LE)  data_len
//! M            data        data_len bytes
//! ```
//!
//! - **HASH** — a content address: `data` MUST be exactly 32 bytes (a SHA-256
//!   digest of externally stored bytes). The prover supplies those bytes to
//!   the guest out of band and the guest MUST verify `sha256(bytes) == data`
//!   before trusting them. (The launch predicates embed their parameters as
//!   VALUE entries instead, so their guest reads only `(image_id,
//!   manifest_bytes, submission)` — see the predicate crates.)
//! - **VALUE** — a literal pinned parameter embedded directly in the manifest:
//!   `data` is the parameter bytes verbatim (`data_len` MAY be 0). This is how
//!   `frozen_clock` (a literal timestamp) and small predicate parameters
//!   (e.g. the matmul rank bound) are pinned.
//! - **SLOT** — the one late-bound input, `submission`: `data_len` MUST be 0.
//!   Its hash is not known at mint time (the manifest, and thus
//!   `marketParamsHash`, is frozen at `createMarket`; the submission arrives
//!   only at reveal). The guest sets the journal's `submission_hash` from the
//!   actual submission bytes it evaluates; the manifest only reserves the
//!   named slot.
//!
//! ## Canonical-form rules (enforced on parse)
//!
//! The encoding is canonical: exactly one byte string decodes to a given
//! manifest, so two different byte strings can never share a `marketParamsHash`
//! preimage. [`parse`] REJECTS any input that violates:
//!
//! 1. `magic == b"TMF1"`.
//! 2. `entry_count >= 1` and matches the number of entries actually present.
//! 3. Entries appear in **strictly ascending order of `name`** (bytewise),
//!    which also forbids duplicate names.
//! 4. `kind` is one of `0x01`/`0x02`/`0x03`; HASH ⇒ `data_len == 32`,
//!    SLOT ⇒ `data_len == 0`.
//! 5. `name` is non-empty valid UTF-8.
//! 6. No trailing bytes after the final entry, and no truncation.
//!
//! ## Version tag
//!
//! The 4-byte `magic` doubles as the version tag: `TMF1` is manifest-v1.
//! Like the journal (see `journal` crate), the version is also bound
//! out-of-band — the market's `imageId` transitively pins the exact manifest
//! parser the guest was built with. A new layout means a new magic, new guest
//! images, and a protocol migration.

use sha2::{Digest, Sha256};

/// 4-byte magic / version tag prefixing every canonical manifest.
pub const MAGIC: [u8; 4] = *b"TMF1";

/// Spec version of the canonical encoding (matches the `1` in `TMF1`).
pub const ENCODING_VERSION: u8 = 1;

/// Entry kind discriminants (the `kind` byte).
pub mod kind {
    /// Content-addressed input: 32-byte SHA-256 digest of external bytes.
    pub const HASH: u8 = 0x01;
    /// Literal pinned parameter embedded in the manifest.
    pub const VALUE: u8 = 0x02;
    /// Late-bound slot (the submission); zero-length data.
    pub const SLOT: u8 = 0x03;
}

/// A single named manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The handle the guest program uses to request the input.
    pub name: String,
    /// The entry payload (hash, literal value, or late-bound slot).
    pub kind: EntryKind,
}

/// The three ways an input is pinned in a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// Content address: SHA-256 of externally stored bytes.
    Hash([u8; 32]),
    /// Literal bytes embedded directly in the manifest.
    Value(Vec<u8>),
    /// Late-bound slot (the submission); no bytes in the manifest.
    Slot,
}

impl Entry {
    /// A HASH entry content-addressing `digest`.
    pub fn hash(name: impl Into<String>, digest: [u8; 32]) -> Self {
        Entry { name: name.into(), kind: EntryKind::Hash(digest) }
    }

    /// A VALUE entry embedding literal `bytes`.
    pub fn value(name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Entry { name: name.into(), kind: EntryKind::Value(bytes.into()) }
    }

    /// A late-bound SLOT entry (the submission).
    pub fn slot(name: impl Into<String>) -> Self {
        Entry { name: name.into(), kind: EntryKind::Slot }
    }

    fn kind_byte(&self) -> u8 {
        match self.kind {
            EntryKind::Hash(_) => kind::HASH,
            EntryKind::Value(_) => kind::VALUE,
            EntryKind::Slot => kind::SLOT,
        }
    }
}

/// A parsed / assembled manifest. Entries are held in canonical order
/// (strictly ascending by name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    entries: Vec<Entry>,
}

/// Errors from [`encode`] / [`Manifest::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A manifest MUST declare at least one entry.
    Empty,
    /// Two entries share a name (case-sensitive, bytewise).
    DuplicateName(String),
    /// A name is empty or longer than `u16::MAX` bytes.
    BadNameLength(String),
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::Empty => write!(f, "manifest must have at least one entry"),
            EncodeError::DuplicateName(n) => write!(f, "duplicate entry name {n:?}"),
            EncodeError::BadNameLength(n) => {
                write!(f, "entry name {n:?} must be 1..=65535 bytes")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Errors from [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Fewer bytes than the fixed header, or a field ran past the end.
    Truncated,
    /// The 4-byte magic / version tag did not match [`MAGIC`].
    BadMagic,
    /// `entry_count` was 0.
    Empty,
    /// An entry's `kind` byte was not HASH/VALUE/SLOT.
    UnknownKind(u8),
    /// A HASH entry did not carry exactly 32 bytes, or a SLOT carried data.
    BadKindLength { kind: u8, data_len: u32 },
    /// An entry name was empty or not valid UTF-8.
    BadName,
    /// Entries were not in strictly ascending name order (also catches
    /// duplicate names). Non-canonical input is rejected so the encoding is
    /// a bijection.
    NotSorted,
    /// Bytes remained after the declared number of entries.
    TrailingBytes,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ParseError::Truncated => write!(f, "manifest truncated"),
            ParseError::BadMagic => write!(f, "bad manifest magic (expected TMF1)"),
            ParseError::Empty => write!(f, "manifest declares zero entries"),
            ParseError::UnknownKind(k) => write!(f, "unknown entry kind {k:#04x}"),
            ParseError::BadKindLength { kind, data_len } => {
                write!(f, "kind {kind:#04x} has invalid data length {data_len}")
            }
            ParseError::BadName => write!(f, "entry name empty or not UTF-8"),
            ParseError::NotSorted => write!(f, "entries not in strict ascending name order"),
            ParseError::TrailingBytes => write!(f, "trailing bytes after last entry"),
        }
    }
}

impl std::error::Error for ParseError {}

impl Manifest {
    /// Build a manifest from entries in any order. Sorts into canonical order
    /// and rejects duplicate / mis-sized names.
    pub fn new(mut entries: Vec<Entry>) -> Result<Self, EncodeError> {
        if entries.is_empty() {
            return Err(EncodeError::Empty);
        }
        for e in &entries {
            let n = e.name.as_bytes().len();
            if n == 0 || n > u16::MAX as usize {
                return Err(EncodeError::BadNameLength(e.name.clone()));
            }
        }
        entries.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        for w in entries.windows(2) {
            if w[0].name == w[1].name {
                return Err(EncodeError::DuplicateName(w[0].name.clone()));
            }
        }
        Ok(Manifest { entries })
    }

    /// The entries, in canonical (ascending-name) order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry named `name`, if present.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// The literal bytes of a VALUE entry named `name`.
    pub fn value(&self, name: &str) -> Option<&[u8]> {
        match self.get(name)?.kind {
            EntryKind::Value(ref v) => Some(v),
            _ => None,
        }
    }

    /// The 32-byte content address of a HASH entry named `name`.
    pub fn hash_of(&self, name: &str) -> Option<[u8; 32]> {
        match self.get(name)?.kind {
            EntryKind::Hash(h) => Some(h),
            _ => None,
        }
    }

    /// Whether `name` is a late-bound SLOT.
    pub fn is_slot(&self, name: &str) -> bool {
        matches!(self.get(name).map(|e| &e.kind), Some(EntryKind::Slot))
    }

    /// Serialize to canonical `manifest-v1` bytes.
    pub fn encode(&self) -> Vec<u8> {
        // `self.entries` is already canonical (sorted, unique, sized) by
        // construction, so this cannot fail.
        encode(&self.entries).expect("Manifest holds canonical entries")
    }

    /// `sha256(encode())` — the `marketParamsHash` this manifest binds.
    pub fn market_params_hash(&self) -> [u8; 32] {
        sha256(&self.encode())
    }
}

/// Encode `entries` to canonical `manifest-v1` bytes. Sorts into ascending
/// name order and rejects empty/duplicate/oversized names, so the output is
/// always canonical and round-trips through [`parse`].
pub fn encode(entries: &[Entry]) -> Result<Vec<u8>, EncodeError> {
    if entries.is_empty() {
        return Err(EncodeError::Empty);
    }
    // Validate + canonicalize order via a fresh Manifest unless already sorted.
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    for e in &sorted {
        let n = e.name.as_bytes().len();
        if n == 0 || n > u16::MAX as usize {
            return Err(EncodeError::BadNameLength(e.name.clone()));
        }
    }
    sorted.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    for w in sorted.windows(2) {
        if w[0].name == w[1].name {
            return Err(EncodeError::DuplicateName(w[0].name.clone()));
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
    for e in sorted {
        out.push(e.kind_byte());
        let name = e.name.as_bytes();
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name);
        match &e.kind {
            EntryKind::Hash(h) => {
                out.extend_from_slice(&(h.len() as u32).to_le_bytes());
                out.extend_from_slice(h);
            }
            EntryKind::Value(v) => {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v);
            }
            EntryKind::Slot => {
                out.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }
    Ok(out)
}

/// A cursor-free strict reader over the manifest bytes.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::Truncated)?;
        if end > self.b.len() {
            return Err(ParseError::Truncated);
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ParseError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, ParseError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

/// Parse canonical `manifest-v1` bytes. Strict: any non-canonical or
/// malformed input is rejected (see the module-level canonical-form rules).
pub fn parse(bytes: &[u8]) -> Result<Manifest, ParseError> {
    let mut r = Reader { b: bytes, pos: 0 };
    if r.take(4)? != MAGIC {
        return Err(ParseError::BadMagic);
    }
    let count = r.u16()?;
    if count == 0 {
        return Err(ParseError::Empty);
    }
    let mut entries = Vec::with_capacity(count as usize);
    let mut prev_name: Option<Vec<u8>> = None;
    for _ in 0..count {
        let kind = r.u8()?;
        let name_len = r.u16()? as usize;
        if name_len == 0 {
            return Err(ParseError::BadName);
        }
        let name_bytes = r.take(name_len)?;
        // Strict ascending order (also rejects duplicates).
        if let Some(prev) = &prev_name {
            if name_bytes <= prev.as_slice() {
                return Err(ParseError::NotSorted);
            }
        }
        prev_name = Some(name_bytes.to_vec());
        let name = core::str::from_utf8(name_bytes)
            .map_err(|_| ParseError::BadName)?
            .to_string();
        let data_len = r.u32()?;
        let entry_kind = match kind {
            kind::HASH => {
                if data_len != 32 {
                    return Err(ParseError::BadKindLength { kind, data_len });
                }
                let d = r.take(32)?;
                let mut h = [0u8; 32];
                h.copy_from_slice(d);
                EntryKind::Hash(h)
            }
            kind::VALUE => {
                let d = r.take(data_len as usize)?;
                EntryKind::Value(d.to_vec())
            }
            kind::SLOT => {
                if data_len != 0 {
                    return Err(ParseError::BadKindLength { kind, data_len });
                }
                EntryKind::Slot
            }
            other => return Err(ParseError::UnknownKind(other)),
        };
        entries.push(Entry { name, kind: entry_kind });
    }
    if r.pos != bytes.len() {
        return Err(ParseError::TrailingBytes);
    }
    Ok(Manifest { entries })
}

/// SHA-256 over exact bytes — the manifest hash and the shared predicate
/// hashing helper. `hash(&manifest.encode())` is the `marketParamsHash`.
pub fn hash(bytes: &[u8]) -> [u8; 32] {
    sha256(bytes)
}

/// SHA-256 helper (alias of [`hash`], kept for symmetry with `journal::sha256`).
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(bytes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        vectors: Vec<Vector>,
    }

    #[derive(Deserialize)]
    struct Vector {
        name: String,
        encoded: String,
        sha256: String,
        entries: Vec<VEntry>,
    }

    #[derive(Deserialize)]
    struct VEntry {
        name: String,
        kind: String,
        #[serde(default)]
        hash: Option<String>,
        #[serde(default)]
        value_hex: Option<String>,
    }

    fn golden() -> Vec<Vector> {
        let raw = include_str!("../tests/golden_manifest_vectors.json");
        serde_json::from_str::<Fixture>(raw).unwrap().vectors
    }

    fn build(v: &Vector) -> Manifest {
        let entries = v
            .entries
            .iter()
            .map(|e| match e.kind.as_str() {
                "hash" => {
                    let h: [u8; 32] = hex::decode(e.hash.as_ref().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap();
                    Entry::hash(&e.name, h)
                }
                "value" => Entry::value(&e.name, hex::decode(e.value_hex.as_ref().unwrap()).unwrap()),
                "slot" => Entry::slot(&e.name),
                other => panic!("bad kind {other}"),
            })
            .collect();
        Manifest::new(entries).unwrap()
    }

    #[test]
    fn golden_vectors_encode_and_hash() {
        for v in golden() {
            let m = build(&v);
            assert_eq!(hex::encode(m.encode()), v.encoded, "encode: {}", v.name);
            assert_eq!(
                hex::encode(m.market_params_hash()),
                v.sha256,
                "hash: {}",
                v.name
            );
        }
    }

    #[test]
    fn golden_vectors_round_trip_parse() {
        for v in golden() {
            let bytes = hex::decode(&v.encoded).unwrap();
            let m = parse(&bytes).unwrap();
            assert_eq!(m, build(&v), "parse: {}", v.name);
            assert_eq!(m.encode(), bytes, "re-encode: {}", v.name);
        }
    }

    #[test]
    fn entries_are_sorted_regardless_of_input_order() {
        let a = Manifest::new(vec![
            Entry::slot("submission"),
            Entry::value("frozen_clock", vec![1, 2, 3]),
            Entry::value("market_params", vec![0u8; 4]),
        ])
        .unwrap();
        let names: Vec<&str> = a.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["frozen_clock", "market_params", "submission"]);
        // Reordered input encodes identically — canonical.
        let b = Manifest::new(vec![
            Entry::value("market_params", vec![0u8; 4]),
            Entry::slot("submission"),
            Entry::value("frozen_clock", vec![1, 2, 3]),
        ])
        .unwrap();
        assert_eq!(a.encode(), b.encode());
    }

    #[test]
    fn typed_getters() {
        let digest = sha256(b"external params");
        let m = Manifest::new(vec![
            Entry::hash("params", digest),
            Entry::value("clock", 42u64.to_le_bytes().to_vec()),
            Entry::slot("submission"),
        ])
        .unwrap();
        assert_eq!(m.hash_of("params"), Some(digest));
        assert_eq!(m.value("clock"), Some(&42u64.to_le_bytes()[..]));
        assert!(m.is_slot("submission"));
        // Wrong-typed access returns None.
        assert_eq!(m.value("params"), None);
        assert_eq!(m.hash_of("clock"), None);
        assert!(!m.is_slot("params"));
        assert_eq!(m.get("absent"), None);
    }

    #[test]
    fn duplicate_names_rejected() {
        let e = encode(&[
            Entry::value("x", vec![1]),
            Entry::value("x", vec![2]),
        ]);
        assert_eq!(e, Err(EncodeError::DuplicateName("x".into())));
    }

    #[test]
    fn empty_manifest_rejected() {
        assert_eq!(encode(&[]), Err(EncodeError::Empty));
        assert_eq!(Manifest::new(vec![]), Err(EncodeError::Empty));
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let mut good = Manifest::new(vec![Entry::slot("s")]).unwrap().encode();
        good[0] = 0x00;
        assert_eq!(parse(&good), Err(ParseError::BadMagic));
    }

    #[test]
    fn parse_rejects_unsorted_and_duplicate() {
        // Hand-build an unsorted stream: entries "b" then "a".
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        for name in ["b", "a"] {
            bytes.push(kind::SLOT);
            bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        assert_eq!(parse(&bytes), Err(ParseError::NotSorted));

        // Duplicate name "a","a" also caught by the strict-ascending rule.
        let mut dup = Vec::new();
        dup.extend_from_slice(&MAGIC);
        dup.extend_from_slice(&2u16.to_le_bytes());
        for _ in 0..2 {
            dup.push(kind::SLOT);
            dup.extend_from_slice(&1u16.to_le_bytes());
            dup.extend_from_slice(b"a");
            dup.extend_from_slice(&0u32.to_le_bytes());
        }
        assert_eq!(parse(&dup), Err(ParseError::NotSorted));
    }

    #[test]
    fn parse_rejects_bad_kind_lengths() {
        // HASH with 31-byte data.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.push(kind::HASH);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(b"p");
        bytes.extend_from_slice(&31u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 31]);
        assert_eq!(
            parse(&bytes),
            Err(ParseError::BadKindLength { kind: kind::HASH, data_len: 31 })
        );
    }

    #[test]
    fn parse_rejects_trailing_and_truncated() {
        let good = Manifest::new(vec![Entry::value("v", vec![9, 9])]).unwrap().encode();
        let mut trailing = good.clone();
        trailing.push(0xff);
        assert_eq!(parse(&trailing), Err(ParseError::TrailingBytes));
        assert_eq!(parse(&good[..good.len() - 1]), Err(ParseError::Truncated));
        assert_eq!(parse(&[]), Err(ParseError::Truncated));
    }

    #[test]
    fn parse_rejects_unknown_kind() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.push(0x09); // unknown
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(b"p");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(parse(&bytes), Err(ParseError::UnknownKind(0x09)));
    }
}
