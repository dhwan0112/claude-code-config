//! Kitty graphics protocol commands (`ESC _ G … ESC \`), ported from
//! libghostty (`src/terminal/kitty/graphics_*.zig`): parsing, the replies,
//! the images and placements of each screen, deletion, and the cursor
//! movement after a placement. Images are never drawn: only their metadata
//! is kept (ids, numbers, sizes, placements).
//!
//! As libghostty-vt (the `lib` build): only the direct medium (`t=d`) loads,
//! and there is no PNG decoder, so a complete PNG answers "unsupported
//! format".

use std::collections::BTreeMap;

use crate::Terminal;

/// As libghostty: the largest width or height of an image (from Kitty).
const MAX_DIMENSION: u32 = 10_000;
/// As libghostty: the most data one image may hold while it loads, and the
/// most a zlib payload may inflate to.
const MAX_SIZE: usize = 400 * 1024 * 1024;
/// As libghostty's APC handler: the most payload bytes one command carries.
pub(crate) const MAX_PAYLOAD: usize = 65 * 1024 * 1024;
/// libghostty-vt's storage limit for a new terminal (the library default).
const LIMIT_START: usize = 10 * 1000 * 1000;
/// The storage limit after a full reset: libghostty resets the storage to
/// its own default, which is higher.
pub(crate) const LIMIT_RESET: usize = 320 * 1000 * 1000;
/// As libghostty: automatic image ids start half-way through the range.
const FIRST_AUTO_ID: u32 = 2_147_483_647;
/// Not in libghostty: at most this many line feeds after a placement, so a
/// huge `r=` cannot stall the terminal.
const MAX_FEEDS: u32 = 1 << 20;

// ------------------------------------------------------------- commands

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quiet {
    No,
    Ok,
    Failures,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Format {
    Rgb,
    #[default]
    Rgba,
    Png,
}

#[derive(Clone, Copy, Debug, Default)]
struct Transmission {
    format: Format,
    /// `t=`: b'd' direct, b'f' file, b't' temporary file, b's' shared memory.
    medium: u8,
    width: u32,
    height: u32,
    image_id: u32,
    image_number: u32,
    placement_id: u32,
    zlib: bool,
    more: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Display {
    image_id: u32,
    image_number: u32,
    placement_id: u32,
    width: u32,
    height: u32,
    x_offset: u32,
    y_offset: u32,
    columns: u32,
    rows: u32,
    /// `C=1`: the cursor stays.
    cursor_stays: bool,
    virtual_placement: bool,
    parent_id: u32,
    z: i32,
}

#[derive(Clone, Copy, Debug)]
enum Delete {
    All(bool),
    Id { delete: bool, image_id: u32, placement_id: u32 },
    Newest { delete: bool, image_number: u32, placement_id: u32 },
    Cursor(bool),
    Frames,
    Cell { delete: bool, x: u32, y: u32 },
    CellZ { delete: bool, x: u32, y: u32, z: i32 },
    Range { delete: bool, first: u32, last: u32 },
    Column { delete: bool, x: u32 },
    Row { delete: bool, y: u32 },
    Z { delete: bool, z: i32 },
}

#[derive(Clone, Copy, Debug)]
enum Control {
    Query(Transmission),
    Transmit(Transmission),
    TransmitDisplay(Transmission, Display),
    Display(Display),
    Delete(Delete),
    /// `a=f`, `a=a`, `a=c`: parsed, not done.
    Animation,
}

struct Command {
    control: Control,
    quiet: Quiet,
    data: Vec<u8>,
}

impl Command {
    fn transmission(&self) -> Transmission {
        match self.control {
            Control::Query(t) | Control::Transmit(t) | Control::TransmitDisplay(t, _) => t,
            _ => Transmission::default(),
        }
    }
}

/// The keys and values of the control part, as libghostty keeps them: a
/// single non-digit character is its code, anything else a number.
struct Kv([Option<u32>; 256]);

impl Kv {
    fn get(&self, k: u8) -> Option<u32> {
        self.0[k as usize]
    }
}

/// As libghostty's `std.fmt.parseInt` (base 10): an optional sign, digits
/// with `_` between them, within `min..=max`.
fn parse_int(b: &[u8], min: i64, max: i64) -> Option<i64> {
    let (neg, digits) = match b.first()? {
        b'+' => (false, &b[1..]),
        b'-' => (true, &b[1..]),
        _ => (false, b),
    };
    if digits.first().is_none_or(|&c| c == b'_') || digits.last() == Some(&b'_') {
        return None;
    }
    let mut acc: i64 = 0;
    for &c in digits {
        if c == b'_' {
            continue;
        }
        if !c.is_ascii_digit() {
            return None;
        }
        let d = (c - b'0') as i64;
        if acc != 0 {
            acc *= 10;
        } else if neg {
            acc = -d;
            if acc < min {
                return None;
            }
            continue;
        }
        acc = if neg { acc - d } else { acc + d };
        if acc < min || acc > max {
            return None;
        }
    }
    Some(acc)
}

/// The command after `G`, or `None` where libghostty drops it without a
/// reply (bad syntax, a bad value, bad base64, too much data).
fn parse(body: &[u8]) -> Option<Command> {
    #[derive(PartialEq)]
    enum St {
        Key,
        KeyIgnore,
        Value,
        ValueIgnore,
        Data,
    }
    let mut kv = Kv([None; 256]);
    let (mut temp, mut len, mut key) = ([0u8; 11], 0usize, 0u8);
    let mut st = St::Key;
    let mut data: &[u8] = &[];
    // As libghostty: a value is a single non-digit character (its code) or
    // a number; z, H and V are signed.
    let finish = |kv: &mut Kv, temp: &[u8], key: u8| -> Option<()> {
        let v = if temp.len() == 1 && !temp[0].is_ascii_digit() {
            temp[0] as u32
        } else if matches!(key, b'z' | b'H' | b'V') {
            parse_int(temp, i32::MIN as i64, i32::MAX as i64)? as i32 as u32
        } else {
            parse_int(temp, 0, u32::MAX as i64)? as u32
        };
        kv.0[key as usize] = Some(v);
        Some(())
    };
    // As libghostty: a key or value too long for its 11-byte buffer is
    // dropped, and so is everything up to the data.
    let accumulate = |c: u8, temp: &mut [u8; 11], len: &mut usize, st: &mut St, overflow: St| {
        if *len >= temp.len() {
            *st = overflow;
            *len = 0;
        } else {
            temp[*len] = c;
            *len += 1;
        }
    };
    for (i, &c) in body.iter().enumerate() {
        match st {
            St::Key => match c {
                b'=' if len != 1 => {
                    st = St::ValueIgnore;
                    len = 0;
                }
                b'=' => {
                    key = temp[0];
                    len = 0;
                    st = St::Value;
                }
                b';' => {
                    st = St::Data;
                    data = &body[i + 1..];
                    break;
                }
                _ => accumulate(c, &mut temp, &mut len, &mut st, St::KeyIgnore),
            },
            St::KeyIgnore => {
                if c == b'=' {
                    st = St::ValueIgnore;
                }
            }
            St::Value => match c {
                b',' | b';' => {
                    finish(&mut kv, &temp[..len], key)?;
                    len = 0;
                    if c == b',' {
                        st = St::Key;
                    } else {
                        st = St::Data;
                        data = &body[i + 1..];
                        break;
                    }
                }
                _ => accumulate(c, &mut temp, &mut len, &mut st, St::ValueIgnore),
            },
            St::ValueIgnore => match c {
                b',' => st = St::KeyIgnore,
                b';' => {
                    st = St::Data;
                    data = &body[i + 1..];
                    break;
                }
                _ => {}
            },
            St::Data => unreachable!(),
        }
    }
    match st {
        St::Key | St::KeyIgnore => return None,
        St::Value => finish(&mut kv, &temp[..len], key)?,
        St::ValueIgnore | St::Data => {}
    }
    if data.len() > MAX_PAYLOAD {
        return None;
    }
    let action = match kv.get(b'a') {
        None => b't',
        Some(v) => u8::try_from(v).ok()?,
    };
    let control = match action {
        b'q' => Control::Query(transmission(&kv)?),
        b't' => Control::Transmit(transmission(&kv)?),
        b'T' => Control::TransmitDisplay(transmission(&kv)?, display(&kv)?),
        b'p' => Control::Display(display(&kv)?),
        b'd' => Control::Delete(delete(&kv)?),
        b'f' if kv.get(b'X').is_none_or(|v| v <= 1) => Control::Animation,
        b'a' if kv.get(b's').is_none_or(|v| v <= 3) => Control::Animation,
        b'c' if kv.get(b'C').is_none_or(|v| v <= 1) => Control::Animation,
        _ => return None,
    };
    let quiet = match kv.get(b'q') {
        None | Some(0) => Quiet::No,
        Some(1) => Quiet::Ok,
        Some(2) => Quiet::Failures,
        _ => return None,
    };
    let data = if data.is_empty() { Vec::new() } else { base64(data)? };
    Some(Command { control, quiet, data })
}

fn transmission(kv: &Kv) -> Option<Transmission> {
    let mut t = Transmission { medium: b'd', ..Default::default() };
    if let Some(v) = kv.get(b'f') {
        t.format = match v {
            24 => Format::Rgb,
            32 => Format::Rgba,
            100 => Format::Png,
            _ => return None,
        };
    }
    if let Some(v) = kv.get(b't') {
        t.medium = match u8::try_from(v).ok()? {
            c @ (b'd' | b'f' | b't' | b's') => c,
            _ => return None,
        };
    }
    t.width = kv.get(b's').unwrap_or(0);
    t.height = kv.get(b'v').unwrap_or(0);
    t.image_id = kv.get(b'i').unwrap_or(0);
    t.image_number = kv.get(b'I').unwrap_or(0);
    t.placement_id = kv.get(b'p').unwrap_or(0);
    if let Some(v) = kv.get(b'o') {
        if v != b'z' as u32 {
            return None;
        }
        t.zlib = true;
    }
    // As libghostty (and Kitty): `m` counts only for the direct medium.
    if t.medium == b'd' {
        t.more = kv.get(b'm').is_some_and(|v| v > 0);
    }
    Some(t)
}

fn display(kv: &Kv) -> Option<Display> {
    let flag = |k: u8| match kv.get(k) {
        None | Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    };
    Some(Display {
        image_id: kv.get(b'i').unwrap_or(0),
        image_number: kv.get(b'I').unwrap_or(0),
        placement_id: kv.get(b'p').unwrap_or(0),
        width: kv.get(b'w').unwrap_or(0),
        height: kv.get(b'h').unwrap_or(0),
        x_offset: kv.get(b'X').unwrap_or(0),
        y_offset: kv.get(b'Y').unwrap_or(0),
        columns: kv.get(b'c').unwrap_or(0),
        rows: kv.get(b'r').unwrap_or(0),
        cursor_stays: flag(b'C')?,
        virtual_placement: flag(b'U')?,
        parent_id: kv.get(b'P').unwrap_or(0),
        z: kv.get(b'z').unwrap_or(0) as i32,
    })
}

fn delete(kv: &Kv) -> Option<Delete> {
    let what = match kv.get(b'd') {
        None => b'a',
        Some(v) => u8::try_from(v).ok()?,
    };
    let delete = what.is_ascii_uppercase();
    let g = |k: u8| kv.get(k).unwrap_or(0);
    Some(match what.to_ascii_lowercase() {
        b'a' => Delete::All(delete),
        b'i' => Delete::Id { delete, image_id: g(b'i'), placement_id: g(b'p') },
        b'n' => Delete::Newest { delete, image_number: g(b'I'), placement_id: g(b'p') },
        b'c' => Delete::Cursor(delete),
        b'f' => Delete::Frames,
        b'p' => Delete::Cell { delete, x: g(b'x'), y: g(b'y') },
        b'q' => Delete::CellZ { delete, x: g(b'x'), y: g(b'y'), z: g(b'z') as i32 },
        b'r' => {
            let (first, last) = (kv.get(b'x')?, kv.get(b'y')?);
            if first > last {
                return None;
            }
            Delete::Range { delete, first, last }
        }
        b'x' => Delete::Column { delete, x: g(b'x') },
        b'y' => Delete::Row { delete, y: g(b'y') },
        b'z' => Delete::Z { delete, z: g(b'z') as i32 },
        _ => return None,
    })
}

/// As libghostty's base64 (simdutf, forgiving): ASCII whitespace is
/// skipped, padding is optional but checked when there, a single last
/// character is an error, extra bits in the last character are not.
fn base64(input: &[u8]) -> Option<Vec<u8>> {
    let s: Vec<u8> = input.iter().copied().filter(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)).collect();
    let pad = s.iter().rev().take_while(|&&c| c == b'=').count();
    if pad > 2 || (pad > 0 && !s.len().is_multiple_of(4)) {
        return None;
    }
    let body = &s[..s.len() - pad];
    if body.len() % 4 == 1 {
        return None;
    }
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 2);
    for chunk in body.chunks(4) {
        let mut acc = 0u32;
        for &c in chunk {
            acc = acc << 6 | value(c)?;
        }
        acc <<= 6 * (4 - chunk.len() as u32);
        out.extend_from_slice(&acc.to_be_bytes()[1..chunk.len()]);
    }
    Some(out)
}

// ------------------------------------------------------------- replies

#[derive(Clone, Copy, Debug)]
struct Response {
    id: u32,
    number: u32,
    placement: u32,
    message: &'static str,
}

impl Default for Response {
    fn default() -> Response {
        Response { id: 0, number: 0, placement: 0, message: "OK" }
    }
}

impl Response {
    fn error(message: &'static str) -> Response {
        Response { message, ..Default::default() }
    }

    fn ok(&self) -> bool {
        self.message == "OK"
    }

    fn empty(&self) -> bool {
        self.id == 0 && self.number == 0
    }

    /// As libghostty: nothing without an image id or number.
    fn encode(&self) -> Option<String> {
        if self.empty() {
            return None;
        }
        let mut keys = Vec::new();
        if self.id > 0 {
            keys.push(format!("i={}", self.id));
        }
        if self.number > 0 {
            keys.push(format!("I={}", self.number));
        }
        if self.placement > 0 {
            keys.push(format!("p={}", self.placement));
        }
        Some(format!("\x1b_G{};{}\x1b\\", keys.join(","), self.message))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Error {
    NoMemory,
    InvalidData,
    Decompression,
    UnsupportedFormat,
    UnsupportedMedium,
    DimensionsRequired,
    DimensionsTooLarge,
}

impl Error {
    fn message(self) -> &'static str {
        match self {
            Error::NoMemory => "ENOMEM: out of memory",
            Error::InvalidData => "EINVAL: invalid data",
            Error::Decompression => "EINVAL: decompression failed",
            Error::UnsupportedFormat => "EINVAL: unsupported format",
            Error::UnsupportedMedium => "EINVAL: unsupported medium",
            Error::DimensionsRequired => "EINVAL: dimensions required",
            Error::DimensionsTooLarge => "EINVAL: dimensions too large",
        }
    }
}

// ------------------------------------------------------------- storage

#[derive(Clone, Copy, Debug)]
struct Image {
    id: u32,
    number: u32,
    width: u32,
    height: u32,
    /// Bytes of pixel data it would hold.
    len: usize,
    /// When it was stored (newer is larger), as libghostty's generation.
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PlacementId {
    /// Given by the terminal (`p=0`): each display adds one.
    Internal(u32),
    External(u32),
}

#[derive(Clone, Copy, Debug)]
struct Placement {
    image_id: u32,
    id: PlacementId,
    /// Absolute row (see `Storage::top_row`) and column; `None` for a
    /// virtual placement (`U=1`).
    pin: Option<(u64, u16)>,
    x_offset: u32,
    y_offset: u32,
    source_width: u32,
    source_height: u32,
    columns: u32,
    rows: u32,
    z: i32,
}

/// An image whose chunks are still coming (`m=1`).
#[derive(Clone, Debug)]
struct Loading {
    image: Image,
    format: Format,
    zlib: bool,
    /// The payload so far: kept only to inflate it; otherwise counted.
    data: Vec<u8>,
    len: usize,
    display: Option<Display>,
    quiet: Quiet,
    /// The terminal gave the id: no reply (as libghostty's `implicit_id`).
    implicit: bool,
}

impl Loading {
    fn new(cmd: &Command) -> Result<Loading, Error> {
        let t = cmd.transmission();
        let display = match cmd.control {
            Control::TransmitDisplay(_, d) => Some(d),
            _ => None,
        };
        let mut l = Loading {
            image: Image {
                id: t.image_id,
                number: t.image_number,
                width: t.width,
                height: t.height,
                len: 0,
                generation: 0,
            },
            format: t.format,
            zlib: t.zlib,
            data: Vec::new(),
            len: 0,
            display,
            quiet: cmd.quiet,
            implicit: false,
        };
        // As libghostty-vt: files and shared memory are not allowed (and
        // never read here).
        if t.medium != b'd' {
            return Err(Error::UnsupportedMedium);
        }
        l.add(&cmd.data)?;
        Ok(l)
    }

    fn add(&mut self, data: &[u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Ok(());
        }
        if self.len + data.len() > MAX_SIZE {
            return Err(Error::InvalidData);
        }
        if self.zlib {
            self.data.extend_from_slice(data);
        }
        self.len += data.len();
        Ok(())
    }

    /// Checks the image as libghostty does once the last chunk is in.
    fn complete(&self) -> Result<Image, Error> {
        let mut img = self.image;
        let mut len = self.len as u64;
        if self.zlib {
            len = inflated_len(&self.data).ok_or(Error::Decompression)?;
        }
        let bpp = match self.format {
            Format::Png => return Err(Error::UnsupportedFormat),
            Format::Rgb => 3,
            Format::Rgba => 4,
        };
        if img.width == 0 || img.height == 0 {
            return Err(Error::DimensionsRequired);
        }
        if img.width > MAX_DIMENSION || img.height > MAX_DIMENSION {
            return Err(Error::DimensionsTooLarge);
        }
        if len != img.width as u64 * img.height as u64 * bpp {
            return Err(Error::InvalidData);
        }
        img.len = len as usize;
        Ok(img)
    }
}

/// How many bytes a zlib stream inflates to, as libghostty's decoder (Zig's
/// `std.compress.flate`) takes it: the header's method must be deflate with
/// a window of at most 32 KiB (its check bits and dictionary flag are not
/// looked at), and four bytes of Adler-32 must follow the data (their value
/// is not checked either); anything after them is ignored. `None`: it does
/// not inflate.
fn inflated_len(data: &[u8]) -> Option<u64> {
    use miniz_oxide::inflate::core::inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    use miniz_oxide::inflate::core::{decompress, DecompressorOxide};
    use miniz_oxide::inflate::TINFLStatus;
    let [cmf, _, rest @ ..] = data else { return None };
    if cmf & 0x0f != 8 || cmf >> 4 > 7 {
        return None;
    }
    let mut d = DecompressorOxide::new();
    // A wrapping window: only the length is wanted. Until the window is full
    // it is not wrapping, so that miniz refuses a match reaching back before
    // the start of the output (as libghostty's decoder: E4's "z-far-dist";
    // in a wrapping window miniz would read the window's zeros).
    let mut out = vec![0u8; 32 * 1024];
    let (mut input, mut pos, mut total) = (rest, 0usize, 0u64);
    loop {
        let flags = if total < out.len() as u64 { TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF } else { 0 };
        let (status, used, made) = decompress(&mut d, input, &mut out, pos, flags);
        input = &input[used..];
        total += made as u64;
        pos = (pos + made) & (out.len() - 1);
        if total > MAX_SIZE as u64 {
            return None;
        }
        match status {
            TINFLStatus::Done => break,
            TINFLStatus::HasMoreOutput => {}
            _ => return None,
        }
    }
    (input.len() >= 4).then_some(total)
}

/// One screen's images and placements (libghostty's `ImageStorage`).
#[derive(Clone, Debug)]
pub(crate) struct Storage {
    images: BTreeMap<u32, Image>,
    /// By image id and placement id.
    placements: BTreeMap<(u32, PlacementId), Placement>,
    loading: Option<Box<Loading>>,
    next_image_id: u32,
    next_internal_id: u32,
    total_bytes: usize,
    pub(crate) limit: usize,
    generation: u64,
    /// The absolute row of the screen's top line: lines scrolled into the
    /// scrollback so far. A placement keeps its absolute row, so it moves up
    /// with the text, as libghostty's pins do.
    pub(crate) top_row: u64,
}

impl Default for Storage {
    fn default() -> Storage {
        Storage {
            images: BTreeMap::new(),
            placements: BTreeMap::new(),
            loading: None,
            next_image_id: FIRST_AUTO_ID,
            next_internal_id: 0,
            total_bytes: 0,
            limit: LIMIT_START,
            generation: 0,
            top_row: 0,
        }
    }
}

impl Storage {
    fn add_image(&mut self, mut img: Image) -> Result<(), Error> {
        if img.len > self.limit {
            return Err(Error::NoMemory);
        }
        let total = self.total_bytes + img.len;
        if total > self.limit && !self.evict(total - self.limit) {
            return Err(Error::NoMemory);
        }
        self.generation += 1;
        img.generation = self.generation;
        if let Some(old) = self.images.insert(img.id, img) {
            self.total_bytes -= old.len;
        }
        self.total_bytes += img.len;
        Ok(())
    }

    /// As libghostty: the oldest images go, unused ones first, until more
    /// than `req` bytes are free; false if that is not reached.
    fn evict(&mut self, req: usize) -> bool {
        let mut order: Vec<(bool, u64, u32)> =
            self.images.values().map(|i| (self.used(i.id), i.generation, i.id)).collect();
        order.sort_unstable();
        let mut evicted = 0;
        for (_, _, id) in order {
            self.placements.retain(|k, _| k.0 != id);
            if let Some(img) = self.images.remove(&id) {
                evicted += img.len;
                self.total_bytes -= img.len;
                if evicted > req {
                    return true;
                }
            }
        }
        false
    }

    fn used(&self, id: u32) -> bool {
        let all = (id, PlacementId::Internal(0))..=(id, PlacementId::External(u32::MAX));
        self.placements.range(all).next().is_some()
    }

    /// The newest image with this number.
    fn by_number(&self, number: u32) -> Option<Image> {
        self.images.values().filter(|i| i.number == number).max_by_key(|i| i.generation).copied()
    }

    fn add_placement(&mut self, mut p: Placement, placement_id: u32) {
        p.id = if placement_id == 0 {
            let id = self.next_internal_id;
            self.next_internal_id = id.wrapping_add(1);
            PlacementId::Internal(id)
        } else {
            PlacementId::External(placement_id)
        };
        self.placements.insert((p.image_id, p.id), p);
    }

    fn delete_if_unused(&mut self, id: u32) {
        if !self.used(id) {
            if let Some(img) = self.images.remove(&id) {
                self.total_bytes -= img.len;
            }
        }
    }

    fn delete_by_id(&mut self, image_id: u32, placement_id: u32, delete: bool) {
        if placement_id == 0 {
            self.placements.retain(|k, _| k.0 != image_id);
        } else {
            self.placements.remove(&(image_id, PlacementId::External(placement_id)));
        }
        if delete {
            self.delete_if_unused(image_id);
        }
    }

    /// Removes the placements `hit` picks; with `delete`, their images too
    /// once unused.
    fn delete_where(&mut self, delete: bool, mut hit: impl FnMut(&Placement, &Image) -> bool) {
        let mut gone = Vec::new();
        let images = &self.images;
        self.placements.retain(|_, p| {
            let Some(img) = images.get(&p.image_id) else { return true };
            let h = hit(p, img);
            if h {
                gone.push(p.image_id);
            }
            !h
        });
        if delete {
            for id in gone {
                self.delete_if_unused(id);
            }
        }
    }

    /// The lines placements are on, counted from the top of the scrollback
    /// (which holds `hist` lines).
    pub(crate) fn pin_lines(&self, hist: usize) -> Vec<usize> {
        let first = self.top_row as i64 - hist as i64;
        let pins = self.placements.values().filter_map(|p| p.pin);
        pins.filter_map(|(row, _)| usize::try_from(row as i64 - first).ok()).collect()
    }

    /// The screen's lines moved by `rows` into (or out of) the scrollback
    /// and its width is now `cols`.
    pub(crate) fn resized(&mut self, rows: i64, cols: u16) {
        self.top_row = self.top_row.saturating_add_signed(rows);
        for p in self.placements.values_mut() {
            if let Some((_, x)) = &mut p.pin {
                *x = (*x).min(cols - 1);
            }
        }
    }

    /// The scrollback was erased: placements in it go to the screen's top
    /// left corner, as libghostty's pins do.
    pub(crate) fn history_erased(&mut self) {
        let top = self.top_row;
        for p in self.placements.values_mut() {
            if let Some(pin) = &mut p.pin {
                if pin.0 < top {
                    *pin = (top, 0);
                }
            }
        }
    }
}

/// The cell size and screen size placements are measured in.
#[derive(Clone, Copy)]
struct Geometry {
    cell: (u32, u32),
    cols: u16,
    rows: u16,
}

impl Geometry {
    fn of(t: &Terminal) -> Geometry {
        Geometry { cell: (t.cell.0 as u32, t.cell.1 as u32), cols: t.cols, rows: t.rows }
    }

    /// As libghostty's `Placement.pixelSize`.
    fn pixel_size(&self, p: &Placement, img: &Image) -> (u32, u32) {
        let w = if p.source_width > 0 { p.source_width } else { img.width };
        let h = if p.source_height > 0 { p.source_height } else { img.height };
        let (cw, ch) = self.cell;
        match (p.columns, p.rows) {
            (0, 0) => (w, h),
            (c, r) if c > 0 && r > 0 => (cw.wrapping_mul(c), ch.wrapping_mul(r)),
            (c, 0) => {
                let width = cw.wrapping_mul(c);
                (width, (width as f64 * (h as f64 / w as f64)).round() as u32)
            }
            (_, r) => {
                let height = ch.wrapping_mul(r);
                ((height as f64 * (w as f64 / h as f64)).round() as u32, height)
            }
        }
    }

    /// As libghostty's `Placement.gridSize`: columns and rows, rounded up.
    fn grid_size(&self, p: &Placement, img: &Image) -> (u32, u32) {
        if p.columns > 0 && p.rows > 0 {
            return (p.columns, p.rows);
        }
        let (w, h) = self.pixel_size(p, img);
        let div = |n: u32, d: u32| if d == 0 { 0 } else { n.div_ceil(d) };
        (div(w.wrapping_add(p.x_offset), self.cell.0), div(h.wrapping_add(p.y_offset), self.cell.1))
    }

    /// As libghostty's `Placement.rect`: the top left and bottom right
    /// cells (absolute row, column); `None` for a virtual placement.
    fn rect(&self, p: &Placement, img: &Image, top_row: u64) -> Option<((u64, u32), (u64, u32))> {
        let (cols, rows) = self.grid_size(p, img);
        let (row, x) = p.pin?;
        let last = top_row + self.rows as u64 - 1;
        let bottom = row.saturating_add(rows.wrapping_sub(1) as u64).min(last);
        let right = (x as u32).wrapping_add(cols.wrapping_sub(1)).min(self.cols as u32 - 1);
        Some(((row, x as u32), (bottom, right)))
    }
}

/// As libghostty's `Pin.isBetween` (within one page): rows after the top
/// row count up to the bottom row whatever their column.
fn between(at: (u64, u32), top: (u64, u32), bottom: (u64, u32)) -> bool {
    if at.0 < top.0 {
        return false;
    }
    if at.0 > top.0 {
        return at.0 <= bottom.0;
    }
    if at.1 < top.1 {
        return false;
    }
    at.0 < bottom.0 || (at.0 == bottom.0 && at.1 <= bottom.1)
}

// ------------------------------------------------------------- execution

/// Runs an APC string (`G` and what follows) and replies as libghostty.
pub(crate) fn apc(t: &mut Terminal, apc: &[u8]) {
    let Some(body) = apc.strip_prefix(b"G") else { return };
    let Some(cmd) = parse(body) else { return };
    let mut quiet = cmd.quiet;
    let resp = match cmd.control {
        Control::Query(tr) => query(&tr),
        Control::Display(d) => display_image(t, &d),
        Control::Delete(d) => {
            delete_placements(t, d);
            Response::default()
        }
        Control::Transmit(_) | Control::TransmitDisplay(..) => {
            // As libghostty: a later chunk without `q` keeps the first
            // chunk's; one with it sets it for the rest.
            if let Some(l) = &mut t.scr().gfx.loading {
                match cmd.quiet {
                    Quiet::No => quiet = l.quiet,
                    q => l.quiet = q,
                }
            }
            transmit(t, &cmd)
        }
        Control::Animation => Response::error("ERROR: unimplemented action"),
    };
    let send = match quiet {
        Quiet::No => !resp.empty(),
        Quiet::Ok => !resp.ok(),
        Quiet::Failures => false,
    };
    if send {
        if let Some(r) = resp.encode() {
            t.reply(r.as_bytes());
        }
    }
}

/// As libghostty: tries to load the image without keeping it.
fn query(tr: &Transmission) -> Response {
    if tr.image_id == 0 {
        return Response::error("EINVAL: image ID required");
    }
    let mut r = Response { id: tr.image_id, number: tr.image_number, placement: tr.placement_id, ..Default::default() };
    if tr.medium != b'd' {
        r.message = Error::UnsupportedMedium.message();
    }
    r
}

fn transmit(t: &mut Terminal, cmd: &Command) -> Response {
    let tr = cmd.transmission();
    let mut r = Response { id: tr.image_id, number: tr.image_number, placement: tr.placement_id, ..Default::default() };
    if tr.image_id > 0 && tr.image_number > 0 {
        return Response::error("EINVAL: image ID and number are mutually exclusive");
    }
    let load = match load_and_add(t, cmd) {
        Ok(l) => l,
        Err(e) => {
            r.message = e.message();
            return r;
        }
    };
    if let Some(mut d) = load.display {
        d.image_id = load.id;
        r = display_image(t, &d);
    }
    // No reply while chunks are still coming, nor for an image the terminal
    // gave its id.
    if load.more || load.implicit {
        return Response::default();
    }
    r.id = load.id;
    r
}

struct Loaded {
    id: u32,
    implicit: bool,
    more: bool,
    display: Option<Display>,
}

fn load_and_add(t: &mut Terminal, cmd: &Command) -> Result<Loaded, Error> {
    let tr = cmd.transmission();
    let s = &mut t.scr().gfx;
    let more = |l: &Loading| Loaded { id: l.image.id, implicit: l.implicit, more: true, display: None };
    let mut loading = match s.loading.take() {
        Some(mut l) => {
            // A later chunk: only its data counts (and its `m`).
            if let Err(e) = l.add(&cmd.data) {
                s.loading = Some(l);
                return Err(e);
            }
            if tr.more {
                let r = more(&l);
                s.loading = Some(l);
                return Ok(r);
            }
            l
        }
        None => Box::new(Loading::new(cmd)?),
    };
    if loading.image.id == 0 {
        loading.image.id = s.next_image_id;
        s.next_image_id = s.next_image_id.wrapping_add(1);
        loading.implicit = loading.image.number == 0;
    }
    if tr.more {
        let r = more(&loading);
        s.loading = Some(loading);
        return Ok(r);
    }
    let img = loading.complete()?;
    s.add_image(img)?;
    Ok(Loaded { id: img.id, implicit: loading.implicit, more: false, display: loading.display })
}

fn display_image(t: &mut Terminal, d: &Display) -> Response {
    if d.image_id == 0 && d.image_number == 0 {
        return Response::error("EINVAL: image ID or number required");
    }
    let mut r = Response { id: d.image_id, number: d.image_number, placement: d.placement_id, ..Default::default() };
    let geo = Geometry::of(t);
    let (cx, cy) = {
        let c = t.scr().cursor;
        (c.x, c.y)
    };
    let s = &mut t.scr().gfx;
    let found = if d.image_id != 0 { s.images.get(&d.image_id).copied() } else { s.by_number(d.image_number) };
    let Some(img) = found else {
        r.message = "ENOENT: image not found";
        return r;
    };
    r.id = img.id;
    let pin = if d.virtual_placement {
        if d.parent_id > 0 {
            r.message = "EINVAL: virtual placement cannot refer to a parent";
            return r;
        }
        None
    } else {
        Some((s.top_row + cy as u64, cx))
    };
    let p = Placement {
        image_id: img.id,
        id: PlacementId::Internal(0),
        pin,
        x_offset: d.x_offset,
        y_offset: d.y_offset,
        source_width: d.width,
        source_height: d.height,
        columns: d.columns,
        rows: d.rows,
        z: d.z,
    };
    s.add_placement(p, d.placement_id);
    // As libghostty: the cursor goes to the image's last row (line feeds
    // scroll as IND does), one column after its right edge.
    if let (Some((_, x)), false) = (pin, d.cursor_stays) {
        let (cols, rows) = geo.grid_size(&p, &img);
        for _ in 0..rows.min(MAX_FEEDS) {
            t.linefeed();
        }
        let y = t.scr().cursor.y as u64;
        set_cursor_pos(t, y, x as u64 + cols as u64 + 1);
    }
    r
}

/// As libghostty's `setCursorPos`: 1-based, inside the region with origin
/// mode.
fn set_cursor_pos(t: &mut Terminal, row: u64, col: u64) {
    let (y_off, y_max) = if t.modes.origin { (t.top as u64, t.bottom as u64 + 1) } else { (0, t.rows as u64) };
    let x = col.max(1).min(t.cols as u64) - 1;
    let y = (row.max(1) + y_off).min(y_max) - 1;
    let c = t.cur();
    c.x = x as u16;
    c.y = y as u16;
    c.pending_wrap = false;
}

fn delete_placements(t: &mut Terminal, d: Delete) {
    let geo = Geometry::of(t);
    let (cx, cy) = {
        let c = t.scr().cursor;
        (c.x as u32, c.y as u32)
    };
    let s = &mut t.scr().gfx;
    let top = s.top_row;
    // A cell of the screen (0-based) as an absolute position, if on it.
    let cell = |x: u32, y: u32| (x < geo.cols as u32 && y < geo.rows as u32).then_some((top + y as u64, x));
    let hits = move |at: (u64, u32)| {
        move |p: &Placement, img: &Image| geo.rect(p, img, top).is_some_and(|(a, b)| between(at, a, b))
    };
    match d {
        Delete::All(delete) => {
            s.delete_where(delete, |p, _| p.pin.is_some());
            if delete {
                let ids: Vec<u32> = s.images.keys().copied().collect();
                for id in ids {
                    s.delete_if_unused(id);
                }
            }
        }
        Delete::Id { delete, image_id, placement_id } => s.delete_by_id(image_id, placement_id, delete),
        Delete::Newest { delete, image_number, placement_id } => {
            if let Some(img) = s.by_number(image_number) {
                s.delete_by_id(img.id, placement_id, delete);
            }
        }
        Delete::Cursor(delete) => {
            if let Some(at) = cell(cx, cy) {
                s.delete_where(delete, hits(at));
            }
        }
        Delete::Cell { delete, x, y } => {
            if let Some(at) = (x > 0 && y > 0).then(|| cell(x - 1, y - 1)).flatten() {
                s.delete_where(delete, hits(at));
            }
        }
        Delete::CellZ { delete, x, y, z } => {
            if let Some(at) = (x > 0 && y > 0).then(|| cell(x - 1, y - 1)).flatten() {
                let hit = hits(at);
                s.delete_where(delete, |p, img| hit(p, img) && p.z == z);
            }
        }
        Delete::Column { delete, x } => {
            if x > 0 {
                let x = x - 1;
                s.delete_where(delete, |p, img| geo.rect(p, img, top).is_some_and(|(a, b)| a.1 <= x && b.1 >= x));
            }
        }
        Delete::Row { delete, y } => {
            if let Some((row, _)) = (y > 0).then(|| cell(0, y - 1)).flatten() {
                s.delete_where(delete, |p, img| geo.rect(p, img, top).is_some_and(|(a, b)| between((row, a.1), a, b)));
            }
        }
        Delete::Z { delete, z } => s.delete_where(delete, |p, _| p.pin.is_some() && p.z == z),
        // As libghostty, whose test is `id >= first or id <= last`: every
        // placement goes.
        Delete::Range { delete, first, last } => {
            if first > 0 && last > 0 {
                s.delete_where(delete, |_, _| true);
            }
        }
        // Not supported, as in libghostty: nothing to delete.
        Delete::Frames => {}
    }
}

/// The scroll region scrolled up by `n` lines that did not go to the
/// scrollback. As libghostty's pins: after IND at its bottom (`index`) the
/// placements in the region move up a row, except on the screen's first row
/// where they go to the first column; after SU only those of the alternate
/// screen's full-screen region move, and not off the first row.
pub(crate) fn region_scrolled(t: &mut Terminal, n: u16, index: bool) {
    let (top, bottom, alt) = (t.top as u64, t.bottom as u64, t.modes.alt_screen);
    // The primary screen's region from the top scrolls into the scrollback
    // (`top_row`).
    let to_scrollback = top == 0 && !alt;
    if to_scrollback || (!index && !(alt && top == 0 && bottom == t.rows as u64 - 1)) {
        return;
    }
    let s = &mut t.scr().gfx;
    let base = s.top_row;
    for p in s.placements.values_mut() {
        let Some((row, x)) = &mut p.pin else { continue };
        for _ in 0..n {
            let Some(y) = row.checked_sub(base).filter(|y| (top..=bottom).contains(y)) else { break };
            if y > 0 {
                *row -= 1;
            } else if index {
                *x = 0;
            }
        }
    }
}

/// The screen is erased (ED 2, ED 22, entering 1049, leaving 1047): its
/// placements go and the images they leave unused (as libghostty).
pub(crate) fn screen_erased(t: &mut Terminal) {
    delete_placements(t, Delete::All(true));
}

#[cfg(test)]
mod tests {
    use crate::{Colors, Options, Terminal};

    fn term(cols: u16, rows: u16) -> Terminal {
        Terminal::new(Options {
            cols,
            rows,
            cell_width: 9,
            cell_height: 18,
            colors: Colors { fg: [204; 3], bg: [20; 3], palette: None, cursor: None },
            dark: Some(true),
            xtversion: "haetty(test)".into(),
            scrollback_limit: 1 << 20,
        })
    }

    /// The replies and the cursor after `input` on a 20x4 screen.
    fn run(input: &str) -> (String, (u16, u16)) {
        let mut t = term(20, 4);
        t.feed(input.as_bytes());
        (String::from_utf8(t.take_replies()).unwrap(), t.cursor().unwrap())
    }

    fn replies(input: &str) -> String {
        run(input).0
    }

    // The expected bytes below are libghostty-vt's, from `haetty-vtdiff --probe`.

    #[test]
    fn transmit_and_errors() {
        let cases = [
            ("\x1b_Ga=t,i=10,s=1,v=1,f=24;AAAA\x1b\\", "\x1b_Gi=10;OK\x1b\\"),
            ("\x1b_Ga=t,s=1,v=1,f=24;AAAA\x1b\\", ""),
            ("\x1b_Ga=t,I=7,s=1,v=1,f=24;AAAA\x1b\\", "\x1b_Gi=2147483647,I=7;OK\x1b\\"),
            ("\x1b_Ga=t,i=3,I=7,s=1,v=1,f=24;AAAA\x1b\\", ""),
            ("\x1b_Ga=t,i=3,p=4,s=1,v=1,f=24;AAAA\x1b\\", "\x1b_Gi=3,p=4;OK\x1b\\"),
            ("\x1b_Ga=t,i=3,s=2,v=1,f=24;AAAA\x1b\\", "\x1b_Gi=3;EINVAL: invalid data\x1b\\"),
            ("\x1b_Ga=t,i=3,f=24;AAAA\x1b\\", "\x1b_Gi=3;EINVAL: dimensions required\x1b\\"),
            ("\x1b_Ga=t,i=3,s=10001,v=1,f=24;AAAA\x1b\\", "\x1b_Gi=3;EINVAL: dimensions too large\x1b\\"),
            ("\x1b_Ga=t,i=3,f=100;iVBORw0KGgo=\x1b\\", "\x1b_Gi=3;EINVAL: unsupported format\x1b\\"),
            ("\x1b_Ga=t,i=3,s=1,v=1,f=24,o=z;AAAA\x1b\\", "\x1b_Gi=3;EINVAL: decompression failed\x1b\\"),
            ("\x1b_Ga=t,i=14,s=1,v=1,f=24,o=z;eJxjYGAAAAAEAAE=\x1b\\", "\x1b_Gi=14;OK\x1b\\"),
            ("\x1b_Ga=t,i=15,t=f,f=24,s=1,v=1;L2V0Yy9wYXNzd2Q=\x1b\\", "\x1b_Gi=15;EINVAL: unsupported medium\x1b\\"),
            ("\x1b_Ga=t,i=1,s=-0,v=1,f=24;AAAA\x1b\\", "\x1b_Gi=1;EINVAL: dimensions required\x1b\\"),
            ("\x1b_Ga=t,i=1,s=1__0,v=1,f=24;AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\x1b\\", "\x1b_Gi=1;OK\x1b\\"),
            ("\x1b_Ga=T,I=3,p=9,s=2,v=1,f=24;AAAA\x1b\\", "\x1b_GI=3,p=9;EINVAL: invalid data\x1b\\"),
            ("\x1b_Ga=q,i=7,I=3,p=2;AAAA\x1b\\", "\x1b_Gi=7,I=3,p=2;OK\x1b\\"),
            ("\x1b_Ga=q,i=2,t=f;AA==\x1b\\", "\x1b_Gi=2;EINVAL: unsupported medium\x1b\\"),
            ("\x1b_Ga=f,i=27\x1b\\", ""),
            ("\x1b_Ga=p,i=99,p=3\x1b\\", "\x1b_Gi=99,p=3;ENOENT: image not found\x1b\\"),
            ("\x1b_Ga=p,I=99\x1b\\", "\x1b_GI=99;ENOENT: image not found\x1b\\"),
            ("\x1b_Ga=p,i=21,q=1\x1b\\", "\x1b_Gi=21;ENOENT: image not found\x1b\\"),
            ("\x1b_Ga=p,i=21,q=2\x1b\\", ""),
            ("\x1b_Ga=p,i=5,z=-3\x1b\\", "\x1b_Gi=5;ENOENT: image not found\x1b\\"),
        ];
        for (input, want) in cases {
            assert_eq!(replies(input), want, "{input:?}");
        }
    }

    #[test]
    fn dropped_commands() {
        for input in [
            "\x1b_Ga=q,i=1;A\x1b\\",
            "\x1b_Ga=q,i=1;AA=\x1b\\",
            "\x1b_Ga=q,i=1;AAAA=\x1b\\",
            "\x1b_Ga=q,i=1;-_-_\x1b\\",
            "\x1b_Ga=q,ii=3,i=2\x1b\\",
            "\x1b_Ga=q,i=2,\x1b\\",
            "\x1b_Ga=q,i=,i=2\x1b\\",
            "\x1b_Ga=q,i=000000000001\x1b\\",
            "\x1b_Ga=q,i=2,t=x\x1b\\",
            "\x1b_Ga=q,i=1,z=2147483648\x1b\\",
            "\x1b_Ga=q,i=1;AA\x0bAA\x1b\\",
            "\x1b_G\x1b\\",
        ] {
            assert_eq!(replies(input), "", "{input:?}");
        }
        // Whitespace in the payload is skipped; CAN ends the string and it runs.
        assert_eq!(replies("\x1b_Ga=q,i=1;AA\nAA\x1b\\"), "\x1b_Gi=1;OK\x1b\\");
        assert_eq!(replies("\x1b_Ga=q,i=1;AA\x18"), "\x1b_Gi=1;OK\x1b\\");
        // SOS and PM strings are read as APC ones.
        assert_eq!(replies("\x1bXGa=q,i=1;AAAA\x1b\\"), "\x1b_Gi=1;OK\x1b\\");
        assert_eq!(replies("\x1b_Ga=q,i=1_0\x1b\\"), "\x1b_Gi=10;OK\x1b\\");
        assert_eq!(replies("\x1b_Ga=q,i=x\x1b\\"), "\x1b_Gi=120;OK\x1b\\");
    }

    #[test]
    fn chunks() {
        let first = "\x1b_Ga=t,i=13,s=2,v=1,f=24,m=1;AAAA\x1b\\";
        assert_eq!(replies(&format!("{first}\x1b_Gm=0;AAAA\x1b\\")), "\x1b_Gi=13;OK\x1b\\");
        let quiet = "\x1b_Ga=t,i=1,s=2,v=1,f=24,m=1,q=1;AAAA\x1b\\";
        assert_eq!(replies(&format!("{quiet}\x1b_Gm=0;AAAA\x1b\\")), "");
        assert_eq!(replies(&format!("{quiet}\x1b_Gi=1,m=0;AA\x1b\\")), "\x1b_Gi=1;EINVAL: invalid data\x1b\\");
        // An error reply names the last chunk's id only.
        assert_eq!(replies(&format!("{first}\x1b_Gm=0;AA\x1b\\")), "");
        let t = "\x1b_Ga=T,i=13,s=2,v=1,f=24,m=1,c=3,r=2;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\";
        assert_eq!(run(t), ("\x1b_Gi=13;OK\x1b\\".into(), (3, 1)));
        let t = "\x1b_Ga=T,i=13,p=4,s=2,v=1,f=24,m=1;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\";
        assert_eq!(replies(t), "\x1b_Gi=13,p=4;OK\x1b\\");
        let t = "\x1b_Ga=t,I=13,s=2,v=1,f=24,m=1;AAAA\x1b\\\x1b_Gm=0;AAAA\x1b\\";
        assert_eq!(replies(t), "\x1b_Gi=2147483647;OK\x1b\\");
        // A query between chunks is its own command.
        let t = format!("{first}\x1b_Ga=q,i=7;AAAA\x1b\\\x1b_Gm=0,i=13;AAAA\x1b\\");
        assert_eq!(replies(&t), "\x1b_Gi=7;OK\x1b\\\x1b_Gi=13;OK\x1b\\");
        // Each screen has its own.
        let t = format!("{first}\x1b[?1049h\x1b_Gm=0;AAAA\x1b\\\x1b[?1049l\x1b_Gm=0;AAAA\x1b\\");
        assert_eq!(replies(&t), "\x1b_Gi=13;OK\x1b\\");
    }

    #[test]
    fn display_moves_the_cursor() {
        let img = "\x1b_Ga=t,i=20,s=1,v=1,f=24;AAAA\x1b\\";
        assert_eq!(
            run(&format!("{img}\x1b_Ga=p,i=20\x1b\\")),
            ("\x1b_Gi=20;OK\x1b\\\x1b_Gi=20;OK\x1b\\".into(), (1, 0))
        );
        let cases = [
            ("\x1b_Ga=T,i=11,c=3,r=2,s=1,v=1,f=24;AAAA\x1b\\", (3, 1)),
            ("\x1b_Ga=T,i=11,C=1,c=3,r=2,s=1,v=1,f=24;AAAA\x1b\\", (0, 0)),
            ("\x1b_Ga=T,i=11,U=1,s=1,v=1,f=24;AAAA\x1b\\", (0, 0)),
            ("\x1b[1;19H\x1b_Ga=T,i=11,c=5,r=1,s=1,v=1,f=24;AAAA\x1b\\", (19, 0)),
            ("\x1b[4;1H\x1b_Ga=T,i=11,c=1,r=3,s=1,v=1,f=24;AAAA\x1b\\", (1, 2)),
            ("\x1b[2;3r\x1b[?6h\x1b[1;1H\x1b_Ga=T,i=11,c=1,r=3,s=1,v=1,f=24;AAAA\x1b\\", (1, 2)),
            ("\x1b_Ga=T,i=11,c=2,s=1,v=1,f=24;AAAA\x1b\\", (2, 0)),
            ("\x1b_Ga=T,i=11,r=2,s=1,v=1,f=24;AAAA\x1b\\", (4, 1)),
            ("\x1b_Ga=T,s=1,v=1,f=24;AAAA\x1b\\", (1, 0)),
        ];
        for (input, cursor) in cases {
            assert_eq!(run(input).1, cursor, "{input:?}");
        }
        let virt = "\x1b_Ga=T,i=11,U=1,P=3,s=1,v=1,f=24;AAAA\x1b\\";
        assert_eq!(replies(virt), "\x1b_Gi=11;EINVAL: virtual placement cannot refer to a parent\x1b\\");
        let n = "\x1b_Ga=t,I=20,s=1,v=1,f=24;AAAA\x1b\\";
        let r = replies(&format!("{n}{n}\x1b_Ga=p,I=20,p=7\x1b\\"));
        assert!(r.ends_with("\x1b_Gi=2147483648,I=20,p=7;OK\x1b\\"), "{r:?}");
    }

    #[test]
    fn deletes() {
        let placed = "\x1b_Ga=T,i=22,s=1,v=1,f=24;AAAA\x1b\\";
        let ok = "\x1b_Gi=22;OK\x1b\\";
        let gone = "\x1b_Gi=22;ENOENT: image not found\x1b\\";
        let cases = [
            ("\x1b_Ga=d,d=i,i=22\x1b\\", ok),
            ("\x1b_Ga=d,d=I,i=22\x1b\\", gone),
            ("\x1b_Ga=d,d=a\x1b\\", ok),
            ("\x1b_Ga=d,d=A\x1b\\", gone),
            ("\x1b[H\x1b_Ga=d,d=C\x1b\\", gone),
            ("\x1b_Ga=d,d=C\x1b\\", ok),
            ("\x1b_Ga=d,d=P,x=1,y=1\x1b\\", gone),
            ("\x1b_Ga=d,d=X,x=1\x1b\\", gone),
            ("\x1b_Ga=d,d=Y,y=1\x1b\\", gone),
            ("\x1b_Ga=d,d=Z,z=0\x1b\\", gone),
            ("\x1b_Ga=d,d=R,x=50,y=60\x1b\\", gone),
            ("\x1b_Ga=d,d=F\x1b\\", ok),
            ("\x1b[2J", gone),
            ("\x1b[3J", ok),
            ("\x1b[?1049h\x1b[?1049l", ok),
        ];
        for (del, want) in cases {
            let r = replies(&format!("{placed}{del}\x1b_Ga=p,i=22\x1b\\"));
            assert_eq!(r, format!("{ok}{want}"), "{del:?}");
        }
        // Rows after the top row of the image count whatever their column.
        let wide = "\x1b[1;3H\x1b_Ga=T,i=11,c=5,r=3,s=1,v=1,f=24,C=1;AAAA\x1b\\";
        let r = replies(&format!("{wide}\x1b[3;10H\x1b_Ga=d,d=C\x1b\\\x1b_Ga=p,i=11\x1b\\"));
        assert!(r.ends_with("ENOENT: image not found\x1b\\"), "{r:?}");
        // A placement moves up with the text.
        let r = replies(&format!("{placed}\n\n\n\n\x1b[1;1H\x1b_Ga=d,d=C\x1b\\\x1b_Ga=p,i=22\x1b\\"));
        assert_eq!(r, format!("{ok}{ok}"));
    }

    #[test]
    fn storage_limit_and_eviction() {
        // 1500x1000 RGBA zeros, 6 MB: two of them pass the 10 MB limit.
        let six = deflate_zeros(1500 * 1000 * 4);
        let t6 = |id: u32| format!("\x1b_Ga=t,i={id},s=1500,v=1000,f=32,o=z;{six}\x1b\\");
        let r = replies(&format!("{}{}\x1b_Ga=p,i=1,C=1\x1b\\\x1b_Ga=p,i=2,C=1\x1b\\", t6(1), t6(2)));
        assert_eq!(r, "\x1b_Gi=1;OK\x1b\\\x1b_Gi=2;OK\x1b\\\x1b_Gi=1;ENOENT: image not found\x1b\\\x1b_Gi=2;OK\x1b\\");
        let big = deflate_zeros(2000 * 2000 * 4);
        let t16 = format!("\x1b_Ga=t,i=1,s=2000,v=2000,f=32,o=z;{big}\x1b\\");
        assert_eq!(replies(&t16), "\x1b_Gi=1;ENOMEM: out of memory\x1b\\");
        assert_eq!(replies(&format!("\x1bc{t16}")), "\x1b_Gi=1;OK\x1b\\", "after RIS the limit is 320 MB");
    }

    /// `n` zero bytes as base64 zlib (stored blocks), for the tests.
    fn deflate_zeros(n: usize) -> String {
        let mut z = vec![0x78, 0x01];
        let mut left = n;
        while left > 0 || z.len() == 2 {
            let k = left.min(65535);
            left -= k;
            z.push(u8::from(left == 0));
            z.extend_from_slice(&(k as u16).to_le_bytes());
            z.extend_from_slice(&(!(k as u16)).to_le_bytes());
            z.resize(z.len() + k, 0);
        }
        // Adler-32 of zeros: a = 1, b = n mod 65521.
        let b = (n % 65521) as u32;
        z.extend_from_slice(&((b << 16) | 1).to_be_bytes());
        tests_b64(&z)
    }

    fn tests_b64(b: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in b.chunks(3) {
            let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                s.push(if i <= c.len() { T[(v >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        s
    }
}
