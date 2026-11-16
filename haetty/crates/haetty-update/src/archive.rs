//! Taking the named files out of a release asset (`.tar.gz` for Linux, `.zip` for Windows), in memory, after its
//! sha256 matched the signed manifest. Refused, the whole archive: an entry with a folder in its name, `..`, an
//! absolute path, a link or a device; a named file missing or there twice; an encrypted or zip64 entry; more than
//! [`MAX_FILE`] bytes in a file. Entries not named in the manifest are passed over. miniz_oxide (already in the
//! binary for PNG) inflates; no archive crate.

use miniz_oxide::inflate::{decompress_to_vec_with_limit, TINFLStatus};

/// Largest file taken out (bytes).
pub const MAX_FILE: usize = 128 * 1024 * 1024;

fn bad(why: impl Into<String>) -> String {
    format!("archive refused: {}", why.into())
}

/// A name the archive may use for a file: plain, at the top level.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == ".."
        || name == "."
        || name.contains(':')
        || name.contains('\0')
    {
        return Err(bad(format!("entry {:?}", name.chars().take(80).collect::<String>())));
    }
    Ok(())
}

/// Collects `wanted` files; refuses a second copy.
struct Taken<'a> {
    wanted: &'a [String],
    got: Vec<(String, Vec<u8>)>,
}

impl Taken<'_> {
    fn add(&mut self, name: &str, data: Vec<u8>) -> Result<(), String> {
        if !self.wanted.iter().any(|w| w == name) {
            return Ok(());
        }
        if self.got.iter().any(|(n, _)| n == name) {
            return Err(bad(format!("{name} twice")));
        }
        self.got.push((name.to_string(), data));
        Ok(())
    }

    fn done(self) -> Result<Vec<(String, Vec<u8>)>, String> {
        for w in self.wanted {
            if !self.got.iter().any(|(n, _)| n == w) {
                return Err(bad(format!("{w} missing")));
            }
        }
        Ok(self.got)
    }
}

fn inflate(raw: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    decompress_to_vec_with_limit(raw, limit).map_err(|e| match e.status {
        TINFLStatus::HasMoreOutput => bad("a file is too large"),
        _ => bad("bad deflate data"),
    })
}

/// A `.tar.gz` (one gzip member, ustar entries).
pub fn untar_gz(gz: &[u8], wanted: &[String]) -> Result<Vec<(String, Vec<u8>)>, String> {
    // gzip header (RFC 1952).
    if gz.len() < 18 || gz[0] != 0x1f || gz[1] != 0x8b || gz[2] != 8 {
        return Err(bad("not gzip"));
    }
    let flags = gz[3];
    let mut i = 10;
    if flags & 4 != 0 {
        let n = u16::from_le_bytes([*gz.get(i).ok_or(bad("gzip"))?, *gz.get(i + 1).ok_or(bad("gzip"))?]) as usize;
        i += 2 + n;
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            let z = gz.get(i..).and_then(|r| r.iter().position(|&b| b == 0)).ok_or(bad("gzip"))?;
            i += z + 1;
        }
    }
    if flags & 2 != 0 {
        i += 2;
    }
    let body = gz.get(i..gz.len().saturating_sub(8)).ok_or(bad("gzip"))?;
    let tar = inflate(body, MAX_FILE * 2)?;
    let mut taken = Taken { wanted, got: vec![] };
    let mut off = 0;
    while off + 512 <= tar.len() {
        let h = &tar[off..off + 512];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let field = |a: usize, b: usize| {
            let f = &h[a..b];
            let end = f.iter().position(|&x| x == 0).unwrap_or(f.len());
            std::str::from_utf8(&f[..end]).map_err(|_| bad("entry name"))
        };
        let name = field(0, 100)?;
        let prefix = if &h[257..262] == b"ustar" { field(345, 500)? } else { "" };
        if !prefix.is_empty() {
            return Err(bad(format!("entry {prefix}/{name}")));
        }
        let octal = field(124, 136)?.trim_matches(|c| c == ' ' || c == '\0');
        let size =
            usize::from_str_radix(if octal.is_empty() { "0" } else { octal }, 8).map_err(|_| bad("entry size"))?;
        let kind = h[156];
        let data_at = off + 512;
        let data = tar.get(data_at..data_at.checked_add(size).ok_or(bad("entry size"))?).ok_or(bad("truncated"))?;
        match kind {
            b'0' | 0 => {
                check_name(name)?;
                if size > MAX_FILE {
                    return Err(bad("a file is too large"));
                }
                taken.add(name, data.to_vec())?;
            }
            // Links, devices, folders, pax and GNU long names: release.sh writes plain ustar files only.
            _ => {
                return Err(bad(format!(
                    "entry type {:?} ({})",
                    kind as char,
                    name.chars().take(80).collect::<String>()
                )))
            }
        }
        off = data_at + size.div_ceil(512) * 512;
    }
    taken.done()
}

fn u16_at(b: &[u8], i: usize) -> Result<usize, String> {
    Ok(u16::from_le_bytes(b.get(i..i + 2).ok_or(bad("truncated"))?.try_into().unwrap()) as usize)
}
fn u32_at(b: &[u8], i: usize) -> Result<usize, String> {
    Ok(u32::from_le_bytes(b.get(i..i + 4).ok_or(bad("truncated"))?.try_into().unwrap()) as usize)
}

/// A `.zip` (stored or deflated entries; no zip64, no encryption).
pub fn unzip(zip: &[u8], wanted: &[String]) -> Result<Vec<(String, Vec<u8>)>, String> {
    // End of central directory: the last 22..(22 + 65535) bytes.
    let from = zip.len().saturating_sub(22 + 65535);
    let eocd = (from..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or(bad("not zip"))?;
    let entries = u16_at(zip, eocd + 10)?;
    let mut p = u32_at(zip, eocd + 16)?;
    if p == 0xffff_ffff || entries == 0xffff {
        return Err(bad("zip64"));
    }
    let mut taken = Taken { wanted, got: vec![] };
    for _ in 0..entries {
        if zip.get(p..p + 4) != Some(&[0x50, 0x4b, 0x01, 0x02][..]) {
            return Err(bad("central directory"));
        }
        let made_by_unix = zip.get(p + 5) == Some(&3);
        let flags = u16_at(zip, p + 8)?;
        let method = u16_at(zip, p + 10)?;
        let csize = u32_at(zip, p + 20)?;
        let usize_ = u32_at(zip, p + 24)?;
        let nlen = u16_at(zip, p + 28)?;
        let xlen = u16_at(zip, p + 30)?;
        let clen = u16_at(zip, p + 32)?;
        let ext = u32_at(zip, p + 38)?;
        let local = u32_at(zip, p + 42)?;
        let name = std::str::from_utf8(zip.get(p + 46..p + 46 + nlen).ok_or(bad("truncated"))?)
            .map_err(|_| bad("entry name"))?;
        p += 46 + nlen + xlen + clen;
        if csize == 0xffff_ffff || usize_ == 0xffff_ffff || local == 0xffff_ffff {
            return Err(bad("zip64"));
        }
        check_name(name)?;
        if flags & 1 != 0 {
            return Err(bad("encrypted entry"));
        }
        // A Unix link or device (the mode in the high 16 bits); folders end in '/' and fail check_name.
        let mode = (ext >> 16) as u32 & 0o170000;
        if made_by_unix && mode != 0 && mode != 0o100000 {
            return Err(bad(format!("entry {name} is not a plain file")));
        }
        if usize_ > MAX_FILE {
            return Err(bad("a file is too large"));
        }
        if zip.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04][..]) {
            return Err(bad("local header"));
        }
        let start = local + 30 + u16_at(zip, local + 26)? + u16_at(zip, local + 28)?;
        let raw = zip.get(start..start.checked_add(csize).ok_or(bad("size"))?).ok_or(bad("truncated"))?;
        let data = match method {
            0 => raw.to_vec(),
            8 => inflate(raw, usize_.max(1))?,
            m => return Err(bad(format!("compression method {m}"))),
        };
        if data.len() != usize_ {
            return Err(bad(format!("{name}: size")));
        }
        taken.add(name, data)?;
    }
    taken.done()
}

/// The asset's files by its name's ending.
pub fn extract(file_name: &str, bytes: &[u8], wanted: &[String]) -> Result<Vec<(String, Vec<u8>)>, String> {
    if file_name.ends_with(".tar.gz") {
        untar_gz(bytes, wanted)
    } else if file_name.ends_with(".zip") {
        unzip(bytes, wanted)
    } else {
        Err(bad("unknown archive type"))
    }
}

/// Test archives (tar.gz and zip writers, plain and hostile): the tests here and in `tests/`.
#[cfg(any(test, feature = "test-support"))]
pub mod make {
    use miniz_oxide::deflate::compress_to_vec;

    /// One tar entry: name, type flag, data.
    pub fn tar_gz(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let mut tar = vec![];
        for (name, kind, data) in entries {
            let mut h = [0u8; 512];
            h[..name.len().min(100)].copy_from_slice(&name.as_bytes()[..name.len().min(100)]);
            h[100..108].copy_from_slice(b"0000644\0");
            h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
            h[156] = *kind;
            h[257..263].copy_from_slice(b"ustar\0");
            h[148..156].copy_from_slice(b"        ");
            let sum: u32 = h.iter().map(|&b| b as u32).sum();
            h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            tar.extend_from_slice(&h);
            tar.extend_from_slice(data);
            tar.resize(tar.len().div_ceil(512) * 512, 0);
        }
        tar.resize(tar.len() + 1024, 0);
        let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
        gz.extend(compress_to_vec(&tar, 6));
        gz.extend([0u8; 8]); // CRC and size: not checked (the sha256 of the whole asset is)
        gz
    }

    /// One zip entry: name, Unix mode (0 = none), data (deflated).
    pub fn zip(entries: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut out = vec![];
        let mut central = vec![];
        for (name, mode, data) in entries {
            let c = compress_to_vec(data, 6);
            let off = out.len() as u32;
            out.extend([0x50, 0x4b, 3, 4, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend((c.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(&c);
            central.extend([
                0x50,
                0x4b,
                1,
                2,
                20,
                if *mode != 0 { 3 } else { 0 },
                20,
                0,
                0,
                0,
                8,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ]);
            central.extend((c.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend((mode << 16).to_le_bytes());
            central.extend(off.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let cd = out.len() as u32;
        out.extend(&central);
        out.extend([0x50, 0x4b, 5, 6, 0, 0, 0, 0]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(cd.to_le_bytes());
        out.extend([0, 0]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::make::{tar_gz, zip};
    use super::*;

    fn w(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn takes_the_named_files_and_refuses_escapes() {
        let want = w(&["haetty", "LICENSE"]);
        let ok = tar_gz(&[("haetty", b'0', b"BIN"), ("LICENSE", b'0', b"MIT"), ("extra", b'0', b"x")]);
        assert_eq!(
            untar_gz(&ok, &want).unwrap(),
            vec![("haetty".into(), b"BIN".to_vec()), ("LICENSE".into(), b"MIT".to_vec())]
        );
        let z = zip(&[("haetty", 0o100755, b"BIN"), ("LICENSE", 0, b"MIT")]);
        assert_eq!(unzip(&z, &want).unwrap().len(), 2);
        for hostile in [
            tar_gz(&[("../haetty", b'0', b"x"), ("LICENSE", b'0', b"")]),
            tar_gz(&[("/etc/haetty", b'0', b"x")]),
            tar_gz(&[("sub/haetty", b'0', b"x")]),
            tar_gz(&[("haetty", b'2', b""), ("LICENSE", b'0', b"")]),
            tar_gz(&[("haetty", b'1', b""), ("LICENSE", b'0', b"")]),
            tar_gz(&[("haetty", b'0', b"a"), ("haetty", b'0', b"b"), ("LICENSE", b'0', b"")]),
            tar_gz(&[("haetty", b'0', b"a")]),
        ] {
            assert!(untar_gz(&hostile, &want).is_err());
        }
        for hostile in [
            zip(&[("../haetty", 0, b"x"), ("LICENSE", 0, b"")]),
            zip(&[("C:\\haetty", 0, b"x")]),
            zip(&[("haetty", 0o120777, b"/etc/passwd"), ("LICENSE", 0, b"")]),
            zip(&[("haetty", 0, b"x")]),
        ] {
            assert!(unzip(&hostile, &want).is_err());
        }
        assert!(extract("x.rar", &ok, &want).is_err());
    }

    /// Any bytes, and the real archives cut or flipped anywhere: no panic.
    #[test]
    fn arbitrary_input_never_panics() {
        let want = w(&["haetty", "LICENSE"]);
        let bases = [
            tar_gz(&[("haetty", b'0', &[7u8; 3000]), ("LICENSE", b'0', b"MIT")]),
            zip(&[("haetty", 0o100755, &[7u8; 3000]), ("LICENSE", 0, b"MIT")]),
        ];
        let mut seed: u64 = 0xdead_beef_1234_5678;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..6000 {
            let base = &bases[round % 2];
            let mut b = base.clone();
            match next() % 3 {
                0 => b.truncate((next() as usize) % b.len()),
                1 => {
                    for _ in 0..(next() % 8 + 1) {
                        let i = (next() as usize) % b.len();
                        b[i] = next() as u8;
                    }
                }
                _ => b = (0..(next() % 2000)).map(|_| next() as u8).collect(),
            }
            let _ = untar_gz(&b, &want);
            let _ = unzip(&b, &want);
        }
    }
}
