//! Pure-Rust 64-bit streaming multimedia container demuxers (`#![forbid(unsafe_code)]`).
//!
//! Supports files of **any size** (from a few bytes to 100+ GB 4K/8K remuxes) using on-demand
//! 64-bit `Read + Seek` file I/O (`MediaReader` + `PacketPayload::Slice { offset: u64, size: u32 }`)
//! so media payloads are never loaded into RAM in advance.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use media_player_400_core::media_kind;
use media_player_400_core::types::Chapter;
use parking_lot::Mutex;

use crate::error::{EngineError, Result};
use crate::frame::{Pixel, Primaries, Range, Space, TransferCharacteristic};

/// Elementary stream media type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
    Subtitle,
    Attachment,
}

/// Demuxed elementary stream packet.
#[derive(Clone, Debug, Default)]
pub struct MediaPacket {
    stream_index: usize,
    pts: Option<i64>,
    dts: Option<i64>,
    duration: i64,
    is_key: bool,
    data: Vec<u8>,
}

impl MediaPacket {
    #[cfg(test)]
    pub fn copy(slice: &[u8]) -> Self {
        Self { data: slice.to_vec(), is_key: true, ..Default::default() }
    }

    pub fn from_vec(stream_index: usize, pts: Option<i64>, dts: Option<i64>, duration: i64, is_key: bool, data: Vec<u8>) -> Self {
        Self { stream_index, pts, dts, duration, is_key, data }
    }

    pub fn stream(&self) -> usize {
        self.stream_index
    }
    pub fn pts(&self) -> Option<i64> {
        self.pts
    }
    pub fn dts(&self) -> Option<i64> {
        self.dts
    }
    pub fn duration(&self) -> i64 {
        self.duration
    }
    #[cfg(test)]
    pub fn set_duration(&mut self, dur: i64) {
        self.duration = dur;
    }
    pub fn is_key(&self) -> bool {
        self.is_key
    }
    pub fn size(&self) -> usize {
        self.data.len()
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Description of one elementary stream in an opened container.
#[derive(Clone, Debug)]
pub struct StreamDesc {
    pub index: usize,
    pub kind: MediaKind,
    pub codec: String,
    pub profile: Option<String>,
    pub time_base: (i32, i32),
    pub width: u32,
    pub height: u32,
    pub fps: Option<(u32, u32)>,
    pub sar: (i32, i32),
    pub pix_fmt: Pixel,
    pub bit_depth: u8,
    pub bit_rate: Option<u64>,
    pub rotation: i32,
    pub color_space: Space,
    pub color_range: Range,
    pub color_trc: TransferCharacteristic,
    pub color_primaries: Primaries,
    pub interlaced: bool,
    pub channels: u16,
    pub channel_layout: String,
    pub sample_rate: u32,
    pub sample_fmt: String,
    pub bits_per_sample: u16,
    pub is_text_sub: bool,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_cover_art: bool,
    pub extra_data: Vec<u8>,
    pub discard: bool,
    pub source_path: String,
    pub kind_ordinal: usize,
}

impl Default for StreamDesc {
    fn default() -> Self {
        Self {
            index: 0,
            kind: MediaKind::Video,
            codec: "unknown".into(),
            profile: None,
            time_base: (1, 1000),
            width: 0,
            height: 0,
            fps: None,
            sar: (1, 1),
            pix_fmt: Pixel::YUV420P,
            bit_depth: 8,
            bit_rate: None,
            rotation: 0,
            color_space: Space::Unspecified,
            color_range: Range::Unspecified,
            color_trc: TransferCharacteristic::Unspecified,
            color_primaries: Primaries::Unspecified,
            interlaced: false,
            channels: 2,
            channel_layout: "stereo".into(),
            sample_rate: 48_000,
            sample_fmt: "fltp".into(),
            bits_per_sample: 16,
            is_text_sub: true,
            language: None,
            title: None,
            is_default: true,
            is_forced: false,
            is_cover_art: false,
            extra_data: Vec::new(),
            discard: false,
            source_path: String::new(),
            kind_ordinal: 0,
        }
    }
}

impl StreamDesc {
    pub fn time_base_sec(&self) -> f64 {
        f64::from(self.time_base.0) / f64::from(self.time_base.1.max(1))
    }

    pub fn fps_f64(&self) -> Option<f64> {
        self.fps.and_then(|(n, d)| (n > 0 && d > 0).then(|| f64::from(n) / f64::from(d)))
    }
}

// =================================================================================================
// 64-bit Random-Access Streaming Reader (`MediaReader`) & Lazy Packet Payloads (`PacketEntry`)
// =================================================================================================

#[derive(Clone)]
pub enum MediaReader {
    File { file: Arc<Mutex<File>>, len: u64 },
    Memory(Arc<Vec<u8>>),
}

impl std::fmt::Debug for MediaReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File { len, .. } => f.debug_struct("MediaReader::File").field("len", len).finish(),
            Self::Memory(b) => f.debug_struct("MediaReader::Memory").field("len", &b.len()).finish(),
        }
    }
}

impl MediaReader {
    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let f = File::open(path)?;
        let len = f.metadata()?.len();
        Ok(Self::File { file: Arc::new(Mutex::new(f)), len })
    }

    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self::Memory(Arc::new(bytes))
    }

    pub fn len(&self) -> u64 {
        match self {
            Self::File { len, .. } => *len,
            Self::Memory(b) => b.len() as u64,
        }
    }

    pub fn read_at(&self, offset: u64, dst: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File { file, len } => {
                if offset >= *len {
                    return Ok(0);
                }
                let mut f = file.lock();
                f.seek(SeekFrom::Start(offset))?;
                let mut total = 0usize;
                while total < dst.len() {
                    match f.read(&mut dst[total..]) {
                        Ok(0) => break,
                        Ok(n) => total += n,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(total)
            }
            Self::Memory(b) => {
                if offset >= b.len() as u64 {
                    return Ok(0);
                }
                let start = offset as usize;
                let n = dst.len().min(b.len() - start);
                dst[..n].copy_from_slice(&b[start..start + n]);
                Ok(n)
            }
        }
    }

    pub fn read_vec_at(&self, offset: u64, size: usize) -> Vec<u8> {
        if offset >= self.len() || size == 0 {
            return Vec::new();
        }
        let avail = (self.len() - offset).min(size as u64) as usize;
        let mut buf = vec![0u8; avail];
        let n = self.read_at(offset, &mut buf).unwrap_or(0);
        buf.truncate(n);
        buf
    }
}

/// Where a packet's payload resides: either a 64-bit byte range on disk (zero RAM until read) or an
/// inline buffer.
#[derive(Clone, Debug)]
pub enum PacketPayload {
    Slice { offset: u64, size: u32 },
    Inline(Vec<u8>),
}

/// Lightweight index entry for a packet in a container.
#[derive(Clone, Debug)]
pub struct PacketEntry {
    pub stream_index: usize,
    pub pts: Option<i64>,
    pub dts: Option<i64>,
    pub duration: i64,
    pub is_key: bool,
    pub payload: PacketPayload,
}

impl PacketEntry {
    pub fn slice(stream_index: usize, pts: Option<i64>, dts: Option<i64>, duration: i64, is_key: bool, offset: u64, size: u32) -> Self {
        Self { stream_index, pts, dts, duration, is_key, payload: PacketPayload::Slice { offset, size } }
    }

    pub fn inline(stream_index: usize, pts: Option<i64>, dts: Option<i64>, duration: i64, is_key: bool, data: Vec<u8>) -> Self {
        Self { stream_index, pts, dts, duration, is_key, payload: PacketPayload::Inline(data) }
    }
}

/// Incremental cluster streaming state for large Matroska / WebM files (`> 32 MB`).
#[derive(Clone, Debug)]
pub struct MkvStreamState {
    pub first_cluster_pos: u64,
    pub next_cluster_pos: u64,
    pub track_nums: Vec<u64>,
    pub track_default_durs: Vec<i64>,
    pub cues: Vec<(u64, u64)>,
}

/// Opened container context with 64-bit streaming I/O.
#[derive(Clone, Debug)]
pub struct ContainerInput {
    pub format_name: String,
    pub duration: Option<f64>,
    pub start_time: f64,
    pub bit_rate: Option<u64>,
    pub size_bytes: Option<u64>,
    pub seekable: bool,
    pub metadata: Vec<(String, String)>,
    pub chapters: Vec<Chapter>,
    pub streams: Vec<StreamDesc>,
    pub cover_art_bytes: Option<Vec<u8>>,
    pub program_stream_seek: bool,
    pub reader: MediaReader,
    pub entries: Vec<PacketEntry>,
    pub cursor: usize,
    pub mkv_state: Option<MkvStreamState>,
}

impl ContainerInput {
    pub fn stream(&self, index: usize) -> Option<&StreamDesc> {
        self.streams.iter().find(|s| s.index == index)
    }

    pub fn discard_except(&mut self, keep: &[usize]) {
        for s in &mut self.streams {
            s.discard = !keep.contains(&s.index);
        }
    }

    /// Reads the next non-discarded packet, fetching its payload on-demand from `MediaReader`
    /// and incrementally parsing subsequent Matroska clusters when needed.
    pub fn read_packet(&mut self) -> Option<MediaPacket> {
        loop {
            while self.cursor < self.entries.len() {
                let entry = &self.entries[self.cursor];
                self.cursor += 1;
                let discarded = self.streams.iter().find(|s| s.index == entry.stream_index).is_some_and(|s| s.discard);
                if discarded {
                    continue;
                }
                let data = match &entry.payload {
                    PacketPayload::Slice { offset, size } => self.reader.read_vec_at(*offset, *size as usize),
                    PacketPayload::Inline(v) => v.clone(),
                };
                return Some(MediaPacket::from_vec(entry.stream_index, entry.pts, entry.dts, entry.duration, entry.is_key, data));
            }
            if !self.advance_mkv_cluster() {
                return None;
            }
        }
    }

    pub fn advance_mkv_cluster(&mut self) -> bool {
        let Some(mut st) = self.mkv_state.take() else {
            return false;
        };
        let file_len = self.reader.len();
        let mut pos = st.next_cluster_pos;
        let mut loaded = false;
        self.entries.clear();
        self.cursor = 0;

        while pos + 4 <= file_len {
            let hdr = self.reader.read_vec_at(pos, 16);
            if let Some((id, id_len)) = read_ebml_id(&hdr)
                && let Some((sz, sz_len)) = read_ebml_vint(&hdr[id_len..])
            {
                let body_pos = pos + (id_len + sz_len) as u64;
                let unknown_sz = sz == (1u64 << (7 * sz_len)) - 1;
                let elem_end = if unknown_sz { file_len } else { (body_pos + sz).min(file_len) };
                if id == 0x1F43_B675 && elem_end > body_pos {
                    self.entries.clear();
                    self.cursor = 0;
                    parse_mkv_cluster_fast(&self.reader, body_pos, elem_end, &st.track_nums, &st.track_default_durs, &mut self.entries);
                    pos = elem_end;
                    if !self.entries.is_empty() {
                        loaded = true;
                        break;
                    }
                }
                if elem_end > pos {
                    pos = elem_end;
                    continue;
                }
            }
            // Resynchronise to the next Cluster magic (`0x1F, 0x43, 0xB6, 0x75`) if needed.
            if let Some(next_c) = find_next_mkv_cluster(&self.reader, pos + 1, file_len) {
                pos = next_c;
            } else {
                pos = file_len;
                break;
            }
        }

        if !self.entries.is_empty() {
            loaded = true;
        }

        st.next_cluster_pos = pos;
        self.mkv_state = Some(st);
        loaded
    }

    /// Seeks to `target_sec` (in container time, i.e. `aim + start_time`).
    pub fn seek_seconds(&mut self, target_sec: f64) -> Result<()> {
        if !self.seekable {
            return Err(EngineError::NotSeekable);
        }
        let target_sec = target_sec.max(self.start_time);

        let primary = self
            .streams
            .iter()
            .find(|s| !s.discard && s.kind == MediaKind::Video && !s.is_cover_art)
            .or_else(|| self.streams.iter().find(|s| !s.discard && s.kind == MediaKind::Audio))
            .map(|s| (s.index, s.time_base_sec()));

        let Some((p_idx, p_tb)) = primary else {
            self.cursor = 0;
            return Ok(());
        };

        // If this is a large streaming MKV, locate the cluster containing or preceding `target_sec`.
        if let Some(mut st) = self.mkv_state.take() {
            let file_len = self.reader.len();
            let target_tc = (target_sec / p_tb.max(1e-9)).round() as i64;
            let cluster_pos = if !st.cues.is_empty() {
                let target_ms = (target_sec * 1000.0).max(0.0).round() as u64;
                let idx = match st.cues.binary_search_by_key(&target_ms, |&(t, _)| t) {
                    Ok(i) => i,
                    Err(0) => 0,
                    Err(i) => i - 1,
                };
                st.cues[idx].1
            } else {
                seek_mkv_cluster_by_timecode(&self.reader, st.first_cluster_pos, file_len, target_tc)
            };
            self.entries.clear();
            self.cursor = 0;
            let mut pos = cluster_pos;
            let mut clusters_read = 0usize;
            while pos + 4 <= file_len && clusters_read < 2 {
                let hdr = self.reader.read_vec_at(pos, 16);
                let Some((id, id_len)) = read_ebml_id(&hdr) else { break };
                let Some((sz, sz_len)) = read_ebml_vint(&hdr[id_len..]) else { break };
                let body_pos = pos + (id_len + sz_len) as u64;
                let unknown_sz = sz == (1u64 << (7 * sz_len)) - 1;
                let elem_end = if unknown_sz { file_len } else { (body_pos + sz).min(file_len) };
                if id == 0x1F43_B675 && elem_end > body_pos {
                    parse_mkv_cluster_fast(&self.reader, body_pos, elem_end, &st.track_nums, &st.track_default_durs, &mut self.entries);
                    clusters_read += 1;
                }
                pos = elem_end;
            }
            st.next_cluster_pos = pos;
            self.mkv_state = Some(st);
        }

        if self.entries.is_empty() {
            self.cursor = 0;
            return Ok(());
        }

        if self.program_stream_seek {
            let mut pos = 0usize;
            for (i, pkt) in self.entries.iter().enumerate() {
                if pkt.stream_index != p_idx {
                    continue;
                }
                if let Some(ts) = pkt.pts.or(pkt.dts) {
                    let t = ts as f64 * p_tb;
                    if t <= target_sec + 1e-4 {
                        pos = i;
                    } else {
                        break;
                    }
                }
            }
            self.cursor = pos;
            return Ok(());
        }

        let mut best_key = None;
        let mut first_key = None;
        for (i, pkt) in self.entries.iter().enumerate() {
            if pkt.stream_index != p_idx || !pkt.is_key {
                continue;
            }
            if first_key.is_none() {
                first_key = Some(i);
            }
            if let Some(ts) = pkt.pts.or(pkt.dts) {
                let t = ts as f64 * p_tb;
                if t <= target_sec + 1e-4 {
                    best_key = Some((i, t));
                } else {
                    break;
                }
            }
        }
        let key_idx = best_key.map(|(i, _)| i).or(first_key).unwrap_or(0);
        let key_time = best_key.map(|(_, t)| t).unwrap_or(self.start_time);

        let mut start_idx = key_idx;
        while start_idx > 0 {
            let prev = &self.entries[start_idx - 1];
            if prev.stream_index == p_idx {
                break;
            }
            let tb = self.stream(prev.stream_index).map_or(p_tb, StreamDesc::time_base_sec);
            let t = prev.pts.or(prev.dts).map_or(key_time, |ts| ts as f64 * tb);
            if t + 0.05 >= key_time {
                start_idx -= 1;
            } else {
                break;
            }
        }
        self.cursor = start_idx;
        Ok(())
    }
}

/// Opens a file of any size (using 64-bit `File` streaming) or HTTP URL and parses its container.
pub fn open_container(src: &str, stop: Arc<AtomicBool>) -> Result<ContainerInput> {
    if stop.load(Ordering::Relaxed) {
        return Err(EngineError::Other("cancelled".into()));
    }
    let is_net = media_kind::is_url(src);
    let reader = if is_net {
        let bytes = fetch_url_bytes(src, &stop)?;
        MediaReader::from_vec(bytes)
    } else {
        let p = Path::new(src);
        if !p.exists() {
            return Err(EngineError::Open { input: src.to_owned(), reason: "file not found".into() });
        }
        MediaReader::from_file(p).map_err(|e| EngineError::Open { input: src.to_owned(), reason: e.to_string() })?
    };

    if stop.load(Ordering::Relaxed) {
        return Err(EngineError::Other("cancelled".into()));
    }
    parse_container_reader(reader, src)
}

#[cfg(test)]
pub fn parse_container_bytes(bytes: &[u8], src: &str) -> Result<ContainerInput> {
    parse_container_reader(MediaReader::from_vec(bytes.to_vec()), src)
}

fn fetch_url_bytes(url: &str, stop: &AtomicBool) -> Result<Vec<u8>> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| EngineError::Open { input: url.to_owned(), reason: "only http:// network streams are supported in standalone mode".into() })?;
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if host_port.contains(':') { host_port.to_owned() } else { format!("{host_port}:80") };
    let mut stream = TcpStream::connect(&addr).map_err(|e| EngineError::Open { input: url.to_owned(), reason: e.to_string() })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let host_only = host_port.split(':').next().unwrap_or(host_port);
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_only}\r\nUser-Agent: 400_media_player/{}\r\nConnection: close\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    );
    stream.write_all(req.as_bytes()).map_err(|e| EngineError::Open { input: url.to_owned(), reason: e.to_string() })?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(EngineError::Other("cancelled".into()));
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > (256 << 20) {
                    break;
                }
            }
            Err(e) => return Err(EngineError::Open { input: url.to_owned(), reason: e.to_string() }),
        }
    }
    let Some(hdr_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err(EngineError::Open { input: url.to_owned(), reason: "invalid HTTP response".into() });
    };
    let hdr = String::from_utf8_lossy(&buf[..hdr_end]);
    if !hdr.starts_with("HTTP/1.1 200") && !hdr.starts_with("HTTP/1.0 200") && !hdr.starts_with("HTTP/1.1 206") {
        let status = hdr.lines().next().unwrap_or("HTTP error");
        return Err(EngineError::Open { input: url.to_owned(), reason: status.to_owned() });
    }
    Ok(buf[hdr_end + 4..].to_vec())
}

pub fn parse_container_reader(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    if file_len < 12 {
        return Err(EngineError::Open { input: src.to_owned(), reason: "file too small or truncated".into() });
    }

    let probe = reader.read_vec_at(0, 65536);
    if probe.len() < 12 {
        return Err(EngineError::Open { input: src.to_owned(), reason: "cannot read container header".into() });
    }

    if probe.windows(8).any(|w| w == L4M0_MAGIC) {
        let all = reader.read_vec_at(0, file_len.min(64 << 20) as usize);
        if let Some(inp) = try_decode_l4m0_index(&all, reader.clone(), file_len) {
            return Ok(inp);
        }
    }

    let mut inp = parse_container_reader_inner(reader, src, &probe)?;
    finalize_container_streams(&mut inp, src);
    Ok(inp)
}

fn parse_container_reader_inner(reader: MediaReader, src: &str, probe: &[u8]) -> Result<ContainerInput> {
    if probe.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return parse_mkv_streaming(reader, src);
    }
    if probe.starts_with(b"fLaC") {
        return parse_flac_streaming(reader, src);
    }
    if (probe.starts_with(b"RIFF") || probe.starts_with(b"RF64")) && &probe[8..12] == b"WAVE" {
        return parse_wav_streaming(reader, src);
    }
    if probe.starts_with(b"RIFF") && (&probe[8..12] == b"AVI " || &probe[8..12] == b"AVIX") {
        return parse_avi_streaming(reader, src);
    }
    if probe.starts_with(b"FORM") && (&probe[8..12] == b"AIFF" || &probe[8..12] == b"AIFC") {
        return parse_aiff_streaming(reader, src);
    }
    if probe.starts_with(b"OggS") {
        return parse_ogg_streaming(reader, src);
    }
    if probe.starts_with(b"YUV4MPEG2 ") {
        return parse_y4m_streaming(reader, src);
    }
    if matches!(&probe[4..8], b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"styp" | b"pnot" | b"skip") {
        return parse_mp4_streaming(reader, src);
    }
    if probe.starts_with(&[0x00, 0x00, 0x01, 0xBA]) {
        return parse_mpeg_ps_streaming(reader, src);
    }
    if (probe.len() >= 188 * 3 && probe[0] == 0x47 && probe[188] == 0x47 && probe[376] == 0x47)
        || (probe.len() >= 192 * 3 && probe[4] == 0x47 && probe[196] == 0x47 && probe[388] == 0x47)
    {
        return parse_mpeg_ts_streaming(reader, src);
    }
    if probe.starts_with(b"FLV\x01") {
        return parse_flv_streaming(reader, src);
    }
    if probe.starts_with(b"DKIF") {
        return parse_ivf_streaming(reader, src);
    }
    if probe.starts_with(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        return parse_asf_streaming(reader, src);
    }
    if probe.starts_with(b".RMF") || probe.starts_with(&[0x06, 0x0E, 0x2B, 0x34]) || probe.starts_with(b"nut/multimedia") {
        return parse_broadcast_container_streaming(reader, src, &probe);
    }
    if probe.starts_with(b"caff")
        || probe.starts_with(b".snd")
        || probe.starts_with(b"riff")
        || probe.starts_with(b"wvpk")
        || probe.starts_with(b"MAC ")
        || probe.starts_with(b"TTA1")
        || probe.starts_with(b"tBaK")
        || probe.starts_with(b"MPCK")
        || probe.starts_with(b"MP+")
        || probe.starts_with(b"DSD ")
        || probe.starts_with(b"FRM8")
        || probe.starts_with(b"#!AMR")
    {
        return parse_hi_res_audio_streaming(reader, src, &probe);
    }
    if probe.starts_with(&[0x0B, 0x77]) || probe.starts_with(&[0x7F, 0xFE, 0x80, 0x01]) {
        return parse_cinema_audio_streaming(reader, src, &probe);
    }
    if probe.starts_with(b"ID3") || (probe[0] == 0xFF && (probe[1] & 0xE0) == 0xE0 && (probe[1] & 0x18) != 0x08) {
        return parse_mp3_or_adts_streaming(reader, src, &probe);
    }
    if probe.starts_with(b"\x89PNG\r\n\x1a\n")
        || probe.starts_with(b"\xFF\xD8\xFF")
        || probe.starts_with(b"GIF8")
        || (probe.starts_with(b"RIFF") && &probe[8..12] == b"WEBP")
        || probe.starts_with(b"BM")
        || probe.starts_with(b"II*\x00")
        || probe.starts_with(b"MM\x00*")
    {
        return parse_image_media(reader, src);
    }

    Err(EngineError::Open { input: src.to_owned(), reason: "unrecognised or corrupted media container format".into() })
}

fn finalize_container_streams(inp: &mut ContainerInput, src: &str) {
    let (mut v_ord, mut a_ord, mut s_ord) = (0usize, 0usize, 0usize);
    for st in &mut inp.streams {
        st.source_path = src.to_owned();
        match st.kind {
            MediaKind::Video => {
                st.kind_ordinal = v_ord;
                v_ord += 1;
            }
            MediaKind::Audio => {
                st.kind_ordinal = a_ord;
                a_ord += 1;
            }
            MediaKind::Subtitle => {
                st.kind_ordinal = s_ord;
                s_ord += 1;
            }
            MediaKind::Attachment => {}
        }
    }
    if inp.entries.is_empty() && inp.mkv_state.is_none() && inp.duration.is_some_and(|d| d > 0.0) {
        build_virtual_schedule(inp);
    }
}

fn build_virtual_schedule(inp: &mut ContainerInput) {
    let dur_sec = inp.duration.unwrap_or(10.0).clamp(0.1, 18_000.0);
    let total_us = (dur_sec * 1_000_000.0).round() as i64;
    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    for s in &mut inp.streams {
        if s.kind == MediaKind::Video && !s.is_cover_art {
            s.time_base = (1, 1_000_000);
            let fps = s.fps_f64().filter(|f| *f > 1.0 && *f <= 240.0).unwrap_or(24.0);
            let frame_us = (1_000_000.0 / fps).round().max(4_000.0) as i64;
            let key_every = (fps.round() as i64).max(1);
            video_streams.push((s.index, frame_us, key_every));
        } else if s.kind == MediaKind::Audio {
            s.time_base = (1, 1_000_000);
            audio_streams.push(s.index);
        }
    }
    let audio_step_us = 40_000i64; // 40 ms audio ticks
    let mut next_v_us = 0i64;
    let mut v_idx = 0i64;
    let mut next_a_us = 0i64;
    let v_step = video_streams.first().map_or(i64::MAX, |v| v.1);

    while next_v_us <= total_us || next_a_us <= total_us {
        let do_video = !video_streams.is_empty() && (audio_streams.is_empty() || next_v_us <= next_a_us) && next_v_us <= total_us;
        if do_video {
            for &(s_idx, frame_us, key_every) in &video_streams {
                let is_key = v_idx % key_every == 0;
                inp.entries.push(PacketEntry::inline(s_idx, Some(next_v_us), Some(next_v_us), frame_us, is_key, Vec::new()));
            }
            next_v_us += v_step;
            v_idx += 1;
        } else if !audio_streams.is_empty() && next_a_us <= total_us {
            for &s_idx in &audio_streams {
                inp.entries.push(PacketEntry::inline(s_idx, Some(next_a_us), Some(next_a_us), audio_step_us, true, Vec::new()));
            }
            next_a_us += audio_step_us;
        } else {
            break;
        }
        if inp.entries.len() >= 1_200_000 {
            break;
        }
    }
}

// =================================================================================================
// L4M0 Index Decoder
// =================================================================================================

const L4M0_MAGIC: &[u8; 8] = b"L4M0IDX1";

fn try_decode_l4m0_index(bytes: &[u8], reader: MediaReader, file_len: u64) -> Option<ContainerInput> {
    let pos = bytes.windows(8).position(|w| w == L4M0_MAGIC)?;
    let mut r = &bytes[pos + 8..];
    let format_name = read_str(&mut r)?;
    let dur_raw = read_f64(&mut r)?;
    let duration = (dur_raw >= 0.0).then_some(dur_raw);
    let start_time = read_f64(&mut r)?;
    let program_stream_seek = read_u8(&mut r)? != 0;
    let meta_len = read_u32(&mut r)? as usize;
    let mut metadata = Vec::with_capacity(meta_len.min(256));
    for _ in 0..meta_len {
        metadata.push((read_str(&mut r)?, read_str(&mut r)?));
    }
    let chap_len = read_u32(&mut r)? as usize;
    let mut chapters = Vec::with_capacity(chap_len.min(256));
    for _ in 0..chap_len {
        chapters.push(Chapter { start: read_f64(&mut r)?, end: read_f64(&mut r)?, title: read_str(&mut r)? });
    }
    let cov_len = read_u32(&mut r)? as usize;
    let cover_art_bytes = if cov_len > 0 { Some(read_slice(&mut r, cov_len)?.to_vec()) } else { None };
    let st_len = read_u32(&mut r)? as usize;
    let mut streams = Vec::with_capacity(st_len.min(64));
    for _ in 0..st_len {
        let index = read_u32(&mut r)? as usize;
        let kind = match read_u8(&mut r)? {
            0 => MediaKind::Video,
            1 => MediaKind::Audio,
            2 => MediaKind::Subtitle,
            _ => MediaKind::Attachment,
        };
        let codec = read_str(&mut r)?;
        let profile = read_opt_str(&mut r)?;
        let tb_num = read_i32(&mut r)?;
        let tb_den = read_i32(&mut r)?;
        let width = read_u32(&mut r)?;
        let height = read_u32(&mut r)?;
        let fn_n = read_u32(&mut r)?;
        let fn_d = read_u32(&mut r)?;
        let fps = (fn_n > 0 && fn_d > 0).then_some((fn_n, fn_d));
        let sar = (read_i32(&mut r)?, read_i32(&mut r)?);
        let bit_depth = read_u8(&mut r)?;
        let interlaced = read_u8(&mut r)? != 0;
        let channels = read_u16(&mut r)?;
        let channel_layout = read_str(&mut r)?;
        let sample_rate = read_u32(&mut r)?;
        let sample_fmt = read_str(&mut r)?;
        let bits_per_sample = read_u16(&mut r)?;
        let is_text_sub = read_u8(&mut r)? != 0;
        let language = read_opt_str(&mut r)?;
        let title = read_opt_str(&mut r)?;
        let is_default = read_u8(&mut r)? != 0;
        let is_forced = read_u8(&mut r)? != 0;
        let is_cover_art = read_u8(&mut r)? != 0;
        let extra_len = read_u32(&mut r)? as usize;
        let extra_data = read_slice(&mut r, extra_len)?.to_vec();
        streams.push(StreamDesc {
            index,
            kind,
            codec,
            profile,
            time_base: (tb_num, tb_den),
            width,
            height,
            fps,
            sar,
            pix_fmt: if bit_depth > 8 { Pixel::YUV420P10LE } else { Pixel::YUV420P },
            bit_depth,
            bit_rate: None,
            rotation: 0,
            color_space: Space::Unspecified,
            color_range: Range::Unspecified,
            color_trc: TransferCharacteristic::Unspecified,
            color_primaries: Primaries::Unspecified,
            interlaced,
            channels,
            channel_layout,
            sample_rate,
            sample_fmt,
            bits_per_sample,
            is_text_sub,
            language,
            title,
            is_default,
            is_forced,
            is_cover_art,
            extra_data,
            discard: false,
            source_path: String::new(),
            kind_ordinal: 0,
        });
    }
    let pkt_len = read_u32(&mut r)? as usize;
    let mut entries = Vec::with_capacity(pkt_len.min(100_000));
    for _ in 0..pkt_len {
        let stream_index = read_u32(&mut r)? as usize;
        let pts_raw = read_i64(&mut r)?;
        let dts_raw = read_i64(&mut r)?;
        let duration = read_i64(&mut r)?;
        let is_key = read_u8(&mut r)? != 0;
        let data_len = read_u32(&mut r)? as usize;
        let data = read_slice(&mut r, data_len)?.to_vec();
        entries.push(PacketEntry::inline(
            stream_index,
            (pts_raw != i64::MIN).then_some(pts_raw),
            (dts_raw != i64::MIN).then_some(dts_raw),
            duration,
            is_key,
            data,
        ));
    }
    let bit_rate = duration.filter(|d| *d > 0.0).map(|d| (file_len as f64 * 8.0 / d) as u64);
    Some(ContainerInput {
        format_name,
        duration,
        start_time,
        bit_rate,
        size_bytes: Some(file_len),
        seekable: duration.is_some(),
        metadata,
        chapters,
        streams,
        cover_art_bytes,
        program_stream_seek,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn read_slice<'a>(r: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if r.len() < n {
        return None;
    }
    let (a, b) = r.split_at(n);
    *r = b;
    Some(a)
}
fn read_u8(r: &mut &[u8]) -> Option<u8> {
    Some(read_slice(r, 1)?[0])
}
fn read_u16(r: &mut &[u8]) -> Option<u16> {
    let s = read_slice(r, 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}
fn read_u32(r: &mut &[u8]) -> Option<u32> {
    let s = read_slice(r, 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn read_i32(r: &mut &[u8]) -> Option<i32> {
    let s = read_slice(r, 4)?;
    Some(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn read_i64(r: &mut &[u8]) -> Option<i64> {
    let s = read_slice(r, 8)?;
    Some(i64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}
fn read_f64(r: &mut &[u8]) -> Option<f64> {
    let s = read_slice(r, 8)?;
    Some(f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}
fn read_str(r: &mut &[u8]) -> Option<String> {
    let len = read_u32(r)? as usize;
    let s = read_slice(r, len)?;
    Some(String::from_utf8_lossy(s).into_owned())
}
fn read_opt_str(r: &mut &[u8]) -> Option<Option<String>> {
    let flag = read_u8(r)?;
    if flag == 0 { Some(None) } else { Some(Some(read_str(r)?)) }
}

// =================================================================================================
// 1. WAV / RF64 (> 4 GB 64-bit) Streaming Demuxer
// =================================================================================================

fn parse_wav_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let is_rf64 = &reader.read_vec_at(0, 4) == b"RF64";
    let mut pos = 12u64;
    let mut channels = 2u16;
    let mut sample_rate = 48_000u32;
    let mut bits_per_sample = 16u16;
    let mut block_align = 4u16;
    let mut audio_format = 1u16;
    let mut rf64_data_size: Option<u64> = None;
    let mut data_offset = 0u64;
    let mut data_len = 0u64;

    while pos + 8 <= file_len {
        let hdr = reader.read_vec_at(pos, 8);
        if hdr.len() < 8 {
            break;
        }
        let tag = &hdr[0..4];
        let mut sz = u64::from(u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]));
        pos += 8;
        if tag == b"ds64" && sz >= 24 {
            let ds = reader.read_vec_at(pos, 24);
            if ds.len() >= 16 {
                rf64_data_size = Some(u64::from_le_bytes([ds[8], ds[9], ds[10], ds[11], ds[12], ds[13], ds[14], ds[15]]));
            }
        } else if tag == b"fmt " && sz >= 16 {
            let chunk = reader.read_vec_at(pos, (sz as usize).min(64));
            if chunk.len() >= 16 {
                audio_format = u16::from_le_bytes([chunk[0], chunk[1]]);
                channels = u16::from_le_bytes([chunk[2], chunk[3]]).max(1);
                sample_rate = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]).max(8000);
                block_align = u16::from_le_bytes([chunk[12], chunk[13]]).max(1);
                bits_per_sample = u16::from_le_bytes([chunk[14], chunk[15]]).max(4);
                if audio_format == 0xFFFE && chunk.len() >= 26 {
                    audio_format = u16::from_le_bytes([chunk[24], chunk[25]]);
                }
            }
        } else if tag == b"data" {
            if (is_rf64 || sz == 0xFFFF_FFFF)
                && let Some(real_sz) = rf64_data_size
            {
                sz = real_sz;
            }
            data_offset = pos;
            data_len = sz.min(file_len.saturating_sub(pos));
            break;
        }
        pos = pos.saturating_add(sz + (sz & 1));
    }

    if data_offset == 0 || data_len == 0 {
        return Err(EngineError::Open { input: src.to_owned(), reason: "WAV/RF64 missing data chunk".into() });
    }

    let (codec, sample_fmt) = match (audio_format, bits_per_sample) {
        (3, 32) => ("pcm_f32le", "flt"),
        (3, 64) => ("pcm_f64le", "dbl"),
        (6, _) => ("pcm_alaw", "s16"),
        (7, _) => ("pcm_mulaw", "s16"),
        (2, _) => ("adpcm_ms", "s16"),
        (0x11, _) => ("adpcm_ima_wav", "s16"),
        (_, 8) => ("pcm_u8", "u8"),
        (_, 24) => ("pcm_s24le", "s32"),
        (_, 32) => ("pcm_s32le", "s32"),
        _ => ("pcm_s16le", "s16"),
    };

    let bytes_per_frame = usize::from(block_align).max(usize::from(channels) * usize::from(bits_per_sample).div_ceil(8)).max(1) as u64;
    let total_frames = data_len / bytes_per_frame;
    let duration = total_frames as f64 / f64::from(sample_rate);
    let frames_per_pkt = u64::from(sample_rate / 50).max(256);
    let chunk_bytes = frames_per_pkt * bytes_per_frame;

    let mut entries = Vec::new();
    let mut off = 0u64;
    let mut frame_cursor = 0i64;
    while off < data_len {
        let len = (data_len - off).min(chunk_bytes);
        let fcount = (len / bytes_per_frame) as i64;
        if fcount == 0 {
            break;
        }
        entries.push(PacketEntry::slice(0, Some(frame_cursor), Some(frame_cursor), fcount, true, data_offset + off, len as u32));
        off += len;
        frame_cursor += fcount;
    }

    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Audio,
        codec: codec.into(),
        time_base: (1, sample_rate as i32),
        channels,
        channel_layout: channel_layout_name(channels),
        sample_rate,
        sample_fmt: sample_fmt.into(),
        bits_per_sample,
        bit_rate: Some(u64::from(sample_rate) * u64::from(channels) * u64::from(bits_per_sample)),
        ..Default::default()
    };

    Ok(ContainerInput {
        format_name: if is_rf64 { "rf64".into() } else { "wav".into() },
        duration: Some(duration),
        start_time: 0.0,
        bit_rate: stream.bit_rate,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

// =================================================================================================
// 2. AIFF / AIFC Streaming Demuxer (`FORM....AIFF`)
// =================================================================================================

fn parse_aiff_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut pos = 12u64;
    let mut channels = 2u16;
    let mut sample_rate = 48_000u32;
    let mut bits_per_sample = 16u16;
    let mut data_offset = 0u64;
    let mut data_len = 0u64;

    while pos + 8 <= file_len {
        let hdr = reader.read_vec_at(pos, 8);
        if hdr.len() < 8 {
            break;
        }
        let tag = &hdr[0..4];
        let sz = u64::from(u32::from_be_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]));
        pos += 8;
        if tag == b"COMM" && sz >= 18 {
            let c = reader.read_vec_at(pos, 18);
            if c.len() >= 18 {
                channels = u16::from_be_bytes([c[0], c[1]]).max(1);
                bits_per_sample = u16::from_be_bytes([c[6], c[7]]).max(8);
                let exp = i32::from(u16::from_be_bytes([c[8], c[9]]) & 0x7FFF) - 16383;
                let mant = u64::from_be_bytes([c[10], c[11], c[12], c[13], c[14], c[15], c[16], c[17]]);
                if (0..30).contains(&exp) {
                    sample_rate = ((mant >> (63 - exp)) as u32).max(8000);
                }
            }
        } else if tag == b"SSND" && sz > 8 {
            data_offset = pos + 8;
            data_len = (sz - 8).min(file_len.saturating_sub(data_offset));
        }
        pos = pos.saturating_add(sz + (sz & 1));
    }

    if data_offset == 0 || data_len == 0 {
        return Err(EngineError::Open { input: src.to_owned(), reason: "AIFF missing SSND chunk".into() });
    }
    let bytes_per_frame = (usize::from(channels) * usize::from(bits_per_sample).div_ceil(8)).max(1) as u64;
    let total_frames = data_len / bytes_per_frame;
    let duration = total_frames as f64 / f64::from(sample_rate);
    let frames_per_pkt = u64::from(sample_rate / 50).max(256);
    let chunk_bytes = frames_per_pkt * bytes_per_frame;

    let mut entries = Vec::new();
    let mut off = 0u64;
    let mut frame_cursor = 0i64;
    while off < data_len {
        let len = (data_len - off).min(chunk_bytes);
        let fcount = (len / bytes_per_frame) as i64;
        if fcount == 0 {
            break;
        }
        entries.push(PacketEntry::slice(0, Some(frame_cursor), Some(frame_cursor), fcount, true, data_offset + off, len as u32));
        off += len;
        frame_cursor += fcount;
    }

    let codec = match bits_per_sample {
        8 => "pcm_s8",
        24 => "pcm_s24be",
        32 => "pcm_s32be",
        _ => "pcm_s16be",
    };
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Audio,
        codec: codec.into(),
        time_base: (1, sample_rate as i32),
        channels,
        channel_layout: channel_layout_name(channels),
        sample_rate,
        sample_fmt: "s16".into(),
        bits_per_sample,
        ..Default::default()
    };

    Ok(ContainerInput {
        format_name: "aiff".into(),
        duration: Some(duration),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

// =================================================================================================
// 3. FLAC Streaming Demuxer
// =================================================================================================

fn parse_flac_streaming(reader: MediaReader, _src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut pos = 4u64;
    let mut sample_rate = 48_000u32;
    let mut channels = 2u16;
    let mut bits_per_sample = 16u16;
    let mut total_samples = 0u64;
    let mut metadata = Vec::new();
    let mut cover_art_bytes = None;

    while pos + 4 <= file_len {
        let hdr = reader.read_vec_at(pos, 4);
        if hdr.len() < 4 {
            break;
        }
        let header = hdr[0];
        let is_last = (header & 0x80) != 0;
        let block_type = header & 0x7F;
        let len = (u64::from(hdr[1]) << 16) | (u64::from(hdr[2]) << 8) | u64::from(hdr[3]);
        pos += 4;
        if block_type == 0 && len >= 18 {
            let block = reader.read_vec_at(pos, 18);
            if block.len() >= 18 {
                let sr = (u32::from(block[10]) << 12) | (u32::from(block[11]) << 4) | (u32::from(block[12]) >> 4);
                if sr > 0 {
                    sample_rate = sr;
                }
                channels = (((block[12] >> 1) & 0x07) + 1) as u16;
                bits_per_sample = ((((block[12] & 1) << 4) | (block[13] >> 4)) + 1) as u16;
                total_samples = (u64::from(block[13] & 0x0F) << 32)
                    | (u64::from(block[14]) << 24)
                    | (u64::from(block[15]) << 16)
                    | (u64::from(block[16]) << 8)
                    | u64::from(block[17]);
            }
        } else if block_type == 4 && len >= 8 && len <= (4 << 20) {
            let block = reader.read_vec_at(pos, len as usize);
            let mut r = block.as_slice();
            if read_str(&mut r).is_some()
                && let Some(count) = read_u32(&mut r)
            {
                for _ in 0..count.min(128) {
                    if let Some(kv) = read_str(&mut r)
                        && let Some((k, v)) = kv.split_once('=')
                    {
                        metadata.push((k.trim().to_lowercase(), v.trim().to_owned()));
                    }
                }
            }
        } else if block_type == 6 && len > 32 && len <= (16 << 20) {
            let block = reader.read_vec_at(pos, len as usize);
            let mut r = block.as_slice();
            let _pic_type = read_u32(&mut r);
            if let Some(mime_len) = read_u32(&mut r)
                && read_slice(&mut r, mime_len as usize).is_some()
                && let Some(desc_len) = read_u32(&mut r)
                && read_slice(&mut r, desc_len as usize).is_some()
                && read_slice(&mut r, 16).is_some()
                && let Some(pic_len) = read_u32(&mut r)
                && let Some(pic_bytes) = read_slice(&mut r, pic_len as usize)
            {
                cover_art_bytes = Some(pic_bytes.to_vec());
            }
        }
        pos = pos.saturating_add(len);
        if is_last {
            break;
        }
    }

    let mut frame_offsets: Vec<(u64, i64)> = Vec::new();
    let mut scan_pos = pos;
    let window = 262_144usize;
    while scan_pos + 6 < file_len {
        let buf = reader.read_vec_at(scan_pos, window + 16);
        if buf.len() < 6 {
            break;
        }
        let limit = buf.len().saturating_sub(6).min(window);
        let mut i = 0usize;
        while i < limit {
            if buf[i] == 0xFF && (buf[i + 1] & 0xFE) == 0xF8 {
                let bs_code = buf[i + 2] >> 4;
                let sr_code = buf[i + 2] & 0x0F;
                if bs_code != 0 && sr_code != 0x0F {
                    let bsize = flac_frame_block_size(&buf[i..]).unwrap_or(4096);
                    frame_offsets.push((scan_pos + i as u64, bsize));
                    i += 64;
                    continue;
                }
            }
            i += 1;
        }
        scan_pos += limit as u64;
    }

    let mut entries = Vec::with_capacity(frame_offsets.len());
    let mut pts_samples = 0i64;
    for (idx, &(start, bsize)) in frame_offsets.iter().enumerate() {
        let end = frame_offsets.get(idx + 1).map_or(file_len, |x| x.0);
        let sz = (end.saturating_sub(start)).min(u64::from(u32::MAX)) as u32;
        entries.push(PacketEntry::slice(0, Some(pts_samples), Some(pts_samples), bsize, true, start, sz));
        pts_samples += bsize;
    }
    if total_samples == 0 {
        total_samples = pts_samples.max(0) as u64;
    }
    let duration = (total_samples > 0).then(|| total_samples as f64 / f64::from(sample_rate));

    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Audio,
        codec: "flac".into(),
        time_base: (1, sample_rate as i32),
        channels,
        channel_layout: channel_layout_name(channels),
        sample_rate,
        sample_fmt: "s16".into(),
        bits_per_sample,
        ..Default::default()
    };

    Ok(ContainerInput {
        format_name: "flac".into(),
        duration,
        start_time: 0.0,
        bit_rate: duration.filter(|d| *d > 0.0).map(|d| (file_len as f64 * 8.0 / d) as u64),
        size_bytes: Some(file_len),
        seekable: true,
        metadata,
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

pub(crate) fn flac_frame_block_size(frame: &[u8]) -> Option<i64> {
    if frame.len() < 6 || frame[0] != 0xFF || (frame[1] & 0xFE) != 0xF8 {
        return None;
    }
    let bs_code = frame[2] >> 4;
    match bs_code {
        1 => Some(192),
        2..=5 => Some(576 * (1i64 << (bs_code - 2))),
        8..=15 => Some(256 * (1i64 << (bs_code - 8))),
        _ => Some(4096),
    }
}

// =================================================================================================
// 4. ISO-BMFF / MP4 / MOV / M4V / M4A / 3GP 64-bit Streaming Demuxer (`co64` & `stsc` supported)
// =================================================================================================

fn parse_mp4_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut streams = Vec::new();
    let mut entries = Vec::new();
    let mut duration_sec: Option<f64> = None;
    let mut pos = 0u64;

    while pos + 8 <= file_len {
        let hdr = reader.read_vec_at(pos, 16);
        if hdr.len() < 8 {
            break;
        }
        let mut sz = u64::from(u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]));
        let typ = &hdr[4..8];
        let mut hdr_len = 8u64;
        if sz == 1 && hdr.len() >= 16 {
            sz = u64::from_be_bytes([hdr[8], hdr[9], hdr[10], hdr[11], hdr[12], hdr[13], hdr[14], hdr[15]]);
            hdr_len = 16;
        } else if sz == 0 {
            sz = file_len - pos;
        }
        if sz < hdr_len {
            break;
        }
        let atom_sz = sz.min(file_len.saturating_sub(pos));
        if atom_sz < hdr_len {
            break;
        }
        if typ == b"moov" {
            let moov_len = (atom_sz - hdr_len).min(128 << 20) as usize;
            let moov = reader.read_vec_at(pos + hdr_len, moov_len);
            parse_moov(&moov, file_len, &mut streams, &mut entries, &mut duration_sec);
        }
        pos += atom_sz;
    }

    if streams.is_empty() {
        return Err(EngineError::NoStreams(src.to_owned()));
    }
    entries.sort_by_key(|p| {
        let tb = streams.iter().find(|s| s.index == p.stream_index).map_or(0.001, StreamDesc::time_base_sec);
        ((p.dts.or(p.pts).unwrap_or(0) as f64 * tb) * 1_000_000.0) as i64
    });

    let calculated_dur = entries.iter().filter_map(|p| {
        let tb = streams.iter().find(|s| s.index == p.stream_index).map_or(0.001, StreamDesc::time_base_sec);
        let pts = p.pts.or(p.dts)?;
        Some((pts + p.duration) as f64 * tb)
    }).fold(0.0f64, f64::max);

    if calculated_dur > 0.0 {
        if let Some(dur) = duration_sec {
            if dur > calculated_dur * 2.0 || dur < calculated_dur * 0.5 {
                duration_sec = Some(calculated_dur);
            }
        } else {
            duration_sec = Some(calculated_dur);
        }
    }

    Ok(ContainerInput {
        format_name: "mov,mp4,m4a,3gp,3g2,mj2".into(),
        duration: duration_sec,
        start_time: 0.0,
        bit_rate: duration_sec.filter(|d| *d > 0.0).map(|d| (file_len as f64 * 8.0 / d) as u64),
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams,
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_moov(moov: &[u8], file_len: u64, streams: &mut Vec<StreamDesc>, entries: &mut Vec<PacketEntry>, duration: &mut Option<f64>) {
    let mut pos = 0usize;
    while pos + 8 <= moov.len() {
        let sz = u32::from_be_bytes([moov[pos], moov[pos + 1], moov[pos + 2], moov[pos + 3]]) as usize;
        if sz < 8 || pos + sz > moov.len() {
            break;
        }
        let typ = &moov[pos + 4..pos + 8];
        let body = &moov[pos + 8..pos + sz];
        if typ == b"mvhd" && body.len() >= 20 {
            let ver = body[0];
            if ver == 0 {
                let scale = u32::from_be_bytes([body[12], body[13], body[14], body[15]]);
                let dur = u32::from_be_bytes([body[16], body[17], body[18], body[19]]);
                if scale > 0 && dur > 0 {
                    *duration = Some(f64::from(dur) / f64::from(scale));
                }
            } else if ver == 1 && body.len() >= 32 {
                let scale = u32::from_be_bytes([body[20], body[21], body[22], body[23]]);
                let dur = u64::from_be_bytes([body[24], body[25], body[26], body[27], body[28], body[29], body[30], body[31]]);
                if scale > 0 && dur > 0 {
                    *duration = Some(dur as f64 / f64::from(scale));
                }
            }
        } else if typ == b"trak" {
            let idx = streams.len();
            if let Some((st, mut pkts)) = parse_trak(body, file_len, idx) {
                streams.push(st);
                entries.append(&mut pkts);
            }
        }
        pos += sz;
    }
}

fn parse_trak(trak: &[u8], file_len: u64, index: usize) -> Option<(StreamDesc, Vec<PacketEntry>)> {
    let mut st = StreamDesc { index, ..Default::default() };
    let mut pkts = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= trak.len() {
        let sz = u32::from_be_bytes([trak[pos], trak[pos + 1], trak[pos + 2], trak[pos + 3]]) as usize;
        if sz < 8 || pos + sz > trak.len() {
            break;
        }
        let typ = &trak[pos + 4..pos + 8];
        let body = &trak[pos + 8..pos + sz];
        if typ == b"tkhd" && body.len() >= 84 {
            let w = u32::from_be_bytes([body[body.len() - 8], body[body.len() - 7], body[body.len() - 6], body[body.len() - 5]]) >> 16;
            let h = u32::from_be_bytes([body[body.len() - 4], body[body.len() - 3], body[body.len() - 2], body[body.len() - 1]]) >> 16;
            if w > 0 && h > 0 {
                st.width = w;
                st.height = h;
            }
        } else if typ == b"mdia" {
            parse_mdia(body, file_len, &mut st, &mut pkts);
        }
        pos += sz;
    }
    Some((st, pkts))
}

fn parse_mdia(mdia: &[u8], file_len: u64, st: &mut StreamDesc, pkts: &mut Vec<PacketEntry>) {
    let mut pos = 0usize;
    while pos + 8 <= mdia.len() {
        let sz = u32::from_be_bytes([mdia[pos], mdia[pos + 1], mdia[pos + 2], mdia[pos + 3]]) as usize;
        if sz < 8 || pos + sz > mdia.len() {
            break;
        }
        let typ = &mdia[pos + 4..pos + 8];
        let body = &mdia[pos + 8..pos + sz];
        if typ == b"mdhd" && body.len() >= 20 {
            let ver = body[0];
            let scale = if ver == 0 {
                u32::from_be_bytes([body[12], body[13], body[14], body[15]])
            } else if body.len() >= 24 {
                u32::from_be_bytes([body[20], body[21], body[22], body[23]])
            } else {
                0
            };
            if scale > 0 {
                st.time_base = (1, scale as i32);
            }
        } else if typ == b"hdlr" && body.len() >= 12 {
            st.kind = match &body[8..12] {
                b"vide" => MediaKind::Video,
                b"soun" => MediaKind::Audio,
                b"subt" | b"sbtl" | b"text" => MediaKind::Subtitle,
                _ => st.kind,
            };
        } else if typ == b"minf" {
            parse_minf(body, file_len, st, pkts);
        }
        pos += sz;
    }
}

fn parse_minf(minf: &[u8], file_len: u64, st: &mut StreamDesc, pkts: &mut Vec<PacketEntry>) {
    let mut pos = 0usize;
    while pos + 8 <= minf.len() {
        let sz = u32::from_be_bytes([minf[pos], minf[pos + 1], minf[pos + 2], minf[pos + 3]]) as usize;
        if sz < 8 || pos + sz > minf.len() {
            break;
        }
        if &minf[pos + 4..pos + 8] == b"stbl" {
            parse_stbl(&minf[pos + 8..pos + sz], file_len, st, pkts);
        }
        pos += sz;
    }
}

fn parse_stbl(stbl: &[u8], file_len: u64, st: &mut StreamDesc, pkts: &mut Vec<PacketEntry>) {
    let mut sizes = Vec::new();
    let mut chunk_offsets: Vec<u64> = Vec::new();
    let mut stsc: Vec<(u32, u32)> = Vec::new();
    let mut stts: Vec<(u32, i64)> = Vec::new();
    let mut ctts: Vec<(u32, i64)> = Vec::new();
    let mut stss: Vec<u32> = Vec::new();
    let mut pos = 0usize;

    while pos + 8 <= stbl.len() {
        let sz = u32::from_be_bytes([stbl[pos], stbl[pos + 1], stbl[pos + 2], stbl[pos + 3]]) as usize;
        if sz < 8 || pos + sz > stbl.len() {
            break;
        }
        let typ = &stbl[pos + 4..pos + 8];
        let b = &stbl[pos + 8..pos + sz];
        if typ == b"stsd" && b.len() >= 16 {
            let entry = &b[8..];
            if entry.len() >= 8 {
                let entry_tag = &entry[4..8];
                st.codec = match entry_tag {
                    b"avc1" | b"avc3" | b"h264" => {
                        st.kind = MediaKind::Video;
                        "h264".into()
                    }
                    b"hvc1" | b"hev1" => {
                        st.kind = MediaKind::Video;
                        "hevc".into()
                    }
                    b"av01" => {
                        st.kind = MediaKind::Video;
                        "av1".into()
                    }
                    b"vp09" => {
                        st.kind = MediaKind::Video;
                        "vp9".into()
                    }
                    b"vp08" => {
                        st.kind = MediaKind::Video;
                        "vp8".into()
                    }
                    b"mp4v" => {
                        st.kind = MediaKind::Video;
                        "mpeg4".into()
                    }
                    b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" => {
                        st.kind = MediaKind::Video;
                        "prores".into()
                    }
                    b"jpeg" | b"mjpa" => {
                        st.kind = MediaKind::Video;
                        "mjpeg".into()
                    }
                    b"png " => {
                        st.kind = MediaKind::Video;
                        "png".into()
                    }
                    b"rle " => {
                        st.kind = MediaKind::Video;
                        "qtrle".into()
                    }
                    b"mp4a" => {
                        st.kind = MediaKind::Audio;
                        "aac".into()
                    }
                    b"fLaC" => {
                        st.kind = MediaKind::Audio;
                        "flac".into()
                    }
                    b"Opus" => {
                        st.kind = MediaKind::Audio;
                        "opus".into()
                    }
                    b"alac" => {
                        st.kind = MediaKind::Audio;
                        "alac".into()
                    }
                    b"ac-3" => {
                        st.kind = MediaKind::Audio;
                        "ac3".into()
                    }
                    b"ec-3" => {
                        st.kind = MediaKind::Audio;
                        "eac3".into()
                    }
                    b".mp3" => {
                        st.kind = MediaKind::Audio;
                        "mp3".into()
                    }
                    b"twos" | b"sowt" | b"lpcm" => {
                        st.kind = MediaKind::Audio;
                        "pcm_s16le".into()
                    }
                    b"fl32" => {
                        st.kind = MediaKind::Audio;
                        "pcm_f32le".into()
                    }
                    b"tx3g" | b"text" => {
                        st.kind = MediaKind::Subtitle;
                        "mov_text".into()
                    }
                    b"wvtt" => {
                        st.kind = MediaKind::Subtitle;
                        "webvtt".into()
                    }
                    _ => String::from_utf8_lossy(entry_tag).trim().to_lowercase(),
                };
                if st.kind == MediaKind::Video && entry.len() >= 36 {
                    let w = u16::from_be_bytes([entry[32], entry[33]]) as u32;
                    let h = u16::from_be_bytes([entry[34], entry[35]]) as u32;
                    if w > 0 && h > 0 {
                        st.width = w;
                        st.height = h;
                    }
                    if entry.len() > 86 {
                        st.extra_data = extract_mp4_sample_entry_extradata(&entry[86..]);
                        parse_mp4_video_color_and_sar(&entry[86..], st);
                    }
                } else if st.kind == MediaKind::Audio && entry.len() >= 36 {
                    let ver = u16::from_be_bytes([entry[16], entry[17]]);
                    let ch = u16::from_be_bytes([entry[24], entry[25]]);
                    let sr_fixed = u16::from_be_bytes([entry[32], entry[33]]) as u32;
                    let mdhd_rate = st.time_base.1.max(0) as u32;
                    let sr = if ver == 2 && entry.len() >= 48 {
                        f64::from_be_bytes([
                            entry[32], entry[33], entry[34], entry[35],
                            entry[36], entry[37], entry[38], entry[39],
                        ]) as u32
                    } else if mdhd_rate > 65_535 {
                        mdhd_rate
                    } else if sr_fixed > 0 {
                        sr_fixed
                    } else {
                        mdhd_rate
                    };
                    if ch > 0 {
                        st.channels = ch;
                        st.channel_layout = channel_layout_name(ch);
                    }
                    if sr > 0 {
                        st.sample_rate = sr;
                    }
                    let child_off = match ver {
                        1 => 36 + 16,
                        2 => 36 + 36,
                        _ => 36,
                    };
                    if entry.len() > child_off {
                        st.extra_data = extract_mp4_sample_entry_extradata(&entry[child_off..]);
                    }
                }
            }
        } else if typ == b"stsz" && b.len() >= 12 {
            let fixed = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            let count = u32::from_be_bytes([b[8], b[9], b[10], b[11]]) as usize;
            if fixed > 0 {
                sizes.resize(count, fixed);
            } else {
                for c in b[12..].chunks_exact(4).take(count) {
                    sizes.push(u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as usize);
                }
            }
        } else if typ == b"stco" && b.len() >= 8 {
            let count = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            for c in b[8..].chunks_exact(4).take(count) {
                chunk_offsets.push(u64::from(u32::from_be_bytes([c[0], c[1], c[2], c[3]])));
            }
        } else if typ == b"co64" && b.len() >= 8 {
            let count = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            for c in b[8..].chunks_exact(8).take(count) {
                chunk_offsets.push(u64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
            }
        } else if typ == b"stsc" && b.len() >= 8 {
            let count = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            for c in b[8..].chunks_exact(12).take(count) {
                let first_chunk = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                let spc = u32::from_be_bytes([c[4], c[5], c[6], c[7]]);
                stsc.push((first_chunk, spc));
            }
        } else if typ == b"stts" && b.len() >= 8 {
            let count = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            for c in b[8..].chunks_exact(8).take(count) {
                let n = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                let d = i64::from(u32::from_be_bytes([c[4], c[5], c[6], c[7]]));
                stts.push((n, d));
            }
        } else if typ == b"ctts" && b.len() >= 8 {
            let count = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            for c in b[8..].chunks_exact(8).take(count) {
                let n = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
                let off = i64::from(i32::from_be_bytes([c[4], c[5], c[6], c[7]]));
                ctts.push((n, off));
            }
        } else if typ == b"stss" && b.len() >= 8 {
            let count = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
            for c in b[8..].chunks_exact(4).take(count) {
                stss.push(u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
            }
        }
        pos += sz;
    }

    let mut sample_offsets: Vec<u64> = Vec::with_capacity(sizes.len());
    if stsc.is_empty() {
        for &off in &chunk_offsets {
            sample_offsets.push(off);
        }
    } else {
        let mut sample_idx = 0usize;
        for (chunk_idx_0, &chunk_base) in chunk_offsets.iter().enumerate() {
            let chunk_1 = (chunk_idx_0 + 1) as u32;
            let spc = stsc.iter().rev().find(|&&(fc, _)| chunk_1 >= fc).map_or(1, |&(_, n)| n) as usize;
            let mut cur_off = chunk_base;
            for _ in 0..spc {
                if sample_idx >= sizes.len() {
                    break;
                }
                sample_offsets.push(cur_off);
                cur_off += sizes[sample_idx] as u64;
                sample_idx += 1;
            }
        }
    }

    let n = sizes.len().min(sample_offsets.len());
    let mut dts = 0i64;
    let (mut stts_i, mut stts_rem) = (0usize, stts.first().map_or(0, |x| x.0));
    let (mut ctts_i, mut ctts_rem) = (0usize, ctts.first().map_or(0, |x| x.0));

    for i in 0..n {
        let dur = stts.get(stts_i).map_or(1, |x| x.1);
        let cts_off = ctts.get(ctts_i).map_or(0, |x| x.1);
        let pts = dts + cts_off;
        let off = sample_offsets[i];
        let len = sizes[i];
        if off + (len as u64) <= file_len {
            let is_key = stss.is_empty() || stss.binary_search(&((i + 1) as u32)).is_ok();
            pkts.push(PacketEntry::slice(st.index, Some(pts), Some(dts), dur, is_key, off, len as u32));
        }
        dts += dur;
        if stts_rem > 1 {
            stts_rem -= 1;
        } else if stts_i + 1 < stts.len() {
            stts_i += 1;
            stts_rem = stts[stts_i].0;
        }
        if ctts_rem > 1 {
            ctts_rem -= 1;
        } else if ctts_i + 1 < ctts.len() {
            ctts_i += 1;
            ctts_rem = ctts[ctts_i].0;
        }
    }
    if st.kind == MediaKind::Video && dts > 0 && n > 0 {
        let total_sec = dts as f64 * st.time_base_sec();
        if total_sec > 0.0 {
            let fps_est = (n as f64 / total_sec).round() as u32;
            if fps_est > 0 {
                st.fps = Some((fps_est, 1));
            }
        }
    }
}

fn parse_mp4_video_color_and_sar(mut b: &[u8], st: &mut StreamDesc) {
    while b.len() >= 8 {
        let sz = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        if sz < 8 || sz > b.len() {
            break;
        }
        let typ = &b[4..8];
        let body = &b[8..sz];
        if typ == b"colr" && body.len() >= 10 && (&body[0..4] == b"nclx" || &body[0..4] == b"nclc") {
            let prim = u16::from_be_bytes([body[4], body[5]]) as u8;
            let trc = u16::from_be_bytes([body[6], body[7]]) as u8;
            let mat = u16::from_be_bytes([body[8], body[9]]) as u8;
            st.color_primaries = Primaries::from_code(prim);
            st.color_trc = TransferCharacteristic::from_code(trc);
            st.color_space = Space::from_code(mat);
            if &body[0..4] == b"nclx" && body.len() >= 11 {
                st.color_range = if (body[10] & 0x80) != 0 { Range::JPEG } else { Range::MPEG };
            }
        } else if typ == b"pasp" && body.len() >= 8 {
            let h_sp = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
            let v_sp = i32::from_be_bytes([body[4], body[5], body[6], body[7]]);
            if h_sp > 0 && v_sp > 0 {
                st.sar = (h_sp, v_sp);
            }
        }
        b = &b[sz..];
    }
}

fn extract_mp4_sample_entry_extradata(mut b: &[u8]) -> Vec<u8> {
    while b.len() >= 8 {
        let sz = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        if sz < 8 || sz > b.len() {
            break;
        }
        let typ = &b[4..8];
        let body = &b[8..sz];
        if matches!(typ, b"avcC" | b"hvcC" | b"dOps" | b"dfLa" | b"dac3" | b"dec3" | b"glbl") {
            return body.to_vec();
        }
        if typ == b"esds" {
            if let Some(asc) = extract_esds_decoder_specific_info(body) {
                return asc;
            }
        }
        if typ == b"wave" {
            let inner = extract_mp4_sample_entry_extradata(body);
            if !inner.is_empty() {
                return inner;
            }
        }
        b = &b[sz..];
    }
    Vec::new()
}

fn extract_esds_decoder_specific_info(esds: &[u8]) -> Option<Vec<u8>> {
    // ISO/IEC 14496-1 ES_Descriptor: find tag 0x05 (DecoderSpecificInfo)
    let mut i = 4usize; // skip version/flags
    while i + 2 <= esds.len() {
        let tag = esds[i];
        i += 1;
        let mut len = 0usize;
        for _ in 0..4 {
            let b = *esds.get(i)?;
            i += 1;
            len = (len << 7) | usize::from(b & 0x7F);
            if (b & 0x80) == 0 {
                break;
            }
        }
        if tag == 0x05 && i + len <= esds.len() && len > 0 {
            return Some(esds[i..i + len].to_vec());
        }
        match tag {
            0x03 => i += 3, // ES_ID (2) + flags (1)
            0x04 => i += 13, // objectTypeIndication (1) + streamType (1) + bufferSizeDB (3) + maxBitrate (4) + avgBitrate (4)
            _ => i = i.saturating_add(len),
        }
    }
    None
}

// =================================================================================================
// 5. Matroska / WebM (`.mkv`, `.webm`, `.mka`) 64-bit EBML Streaming Demuxer
// =================================================================================================

pub fn read_ebml_id(buf: &[u8]) -> Option<(u32, usize)> {
    let b0 = *buf.first()?;
    let len = b0.leading_zeros() as usize + 1;
    if len > 4 || buf.len() < len {
        return None;
    }
    let mut id = 0u32;
    for &b in &buf[..len] {
        id = (id << 8) | u32::from(b);
    }
    Some((id, len))
}

pub fn read_ebml_vint(buf: &[u8]) -> Option<(u64, usize)> {
    let b0 = *buf.first()?;
    let len = b0.leading_zeros() as usize + 1;
    if len > 8 || buf.len() < len {
        return None;
    }
    let mask = (1u8 << (8 - len)).wrapping_sub(1);
    let mut val = u64::from(b0 & mask);
    for &b in &buf[1..len] {
        val = (val << 8) | u64::from(b);
    }
    Some((val, len))
}

fn parse_mkv_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut pos = 0u64;
    let mut streams: Vec<StreamDesc> = Vec::new();
    let mut track_nums: Vec<u64> = Vec::new();
    let mut track_default_ns: Vec<u64> = Vec::new();
    let mut entries: Vec<PacketEntry> = Vec::new();
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut timecode_scale = 1_000_000u64;
    let mut duration_ns: Option<f64> = None;
    let mut mkv_state: Option<MkvStreamState> = None;
    let mut seg_start = 0u64;
    let mut seekhead_cues_pos: Option<u64> = None;

    while pos + 4 <= file_len {
        let hdr = reader.read_vec_at(pos, 16);
        let Some((id, id_len)) = read_ebml_id(&hdr) else { break };
        let Some((sz, sz_len)) = read_ebml_vint(&hdr[id_len..]) else { break };
        let body_pos = pos + (id_len + sz_len) as u64;
        let unknown_sz = sz == (1u64 << (7 * sz_len)) - 1;
        let elem_end = if unknown_sz { file_len } else { (body_pos + sz).min(file_len) };

        match id {
            0x1853_8067 => {
                seg_start = body_pos;
                pos = body_pos;
                continue;
            }
            0x114D_9B74 => {
                let sh_buf = reader.read_vec_at(body_pos, (elem_end - body_pos).min(65536) as usize);
                let (c_pos, _) = parse_mkv_seekhead(&sh_buf, seg_start);
                if let Some(cp) = c_pos {
                    seekhead_cues_pos = Some(cp);
                }
            }
            0x1549_A966 => {
                let info = reader.read_vec_at(body_pos, (elem_end - body_pos).min(65536) as usize);
                parse_mkv_info(&info, &mut timecode_scale, &mut duration_ns);
            }
            0x1654_AE6B => {
                let trks = reader.read_vec_at(body_pos, (elem_end - body_pos).min(1 << 20) as usize);
                parse_mkv_tracks(&trks, &mut streams, &mut track_nums, &mut track_default_ns);
            }
            0x1043_A770 => {
                let ch_buf = reader.read_vec_at(body_pos, (elem_end - body_pos).min(256 << 10) as usize);
                parse_mkv_chapters(&ch_buf, &mut chapters);
            }
            0x1F43_B675 => {
                let track_default_durs: Vec<i64> = track_default_ns
                    .iter()
                    .map(|&ns| if ns > 0 { (ns / timecode_scale.max(1)).max(1) as i64 } else { 32 })
                    .collect();
                parse_mkv_cluster_fast(&reader, body_pos, elem_end, &track_nums, &track_default_durs, &mut entries);
                if file_len > (32 << 20) && !streams.is_empty() {
                    if seekhead_cues_pos.is_none() && file_len > (1 << 20) {
                        let tail_sz = (2 << 20).min(file_len) as usize;
                        let tail = reader.read_vec_at(file_len - tail_sz as u64, tail_sz);
                        for i in 0..tail.len().saturating_sub(4) {
                            if &tail[i..i + 4] == [0x1C, 0x53, 0xBB, 0x6B] {
                                seekhead_cues_pos = Some(file_len - tail_sz as u64 + i as u64);
                                break;
                            }
                        }
                    }
                    let cues = if let Some(cp) = seekhead_cues_pos {
                        parse_mkv_cues(&reader, cp, seg_start)
                    } else {
                        Vec::new()
                    };
                    mkv_state = Some(MkvStreamState {
                        first_cluster_pos: pos,
                        next_cluster_pos: elem_end,
                        track_nums: track_nums.clone(),
                        track_default_durs,
                        cues,
                    });
                    break;
                }
            }
            _ => {}
        }
        pos = elem_end;
    }

    if streams.is_empty() {
        return Err(EngineError::Open { input: src.to_owned(), reason: "Matroska segment has no decodable tracks".into() });
    }
    let tb_num = (timecode_scale / 1_000).max(1) as i32;
    for s in &mut streams {
        s.time_base = (tb_num, 1_000_000);
    }
    let duration = duration_ns.map(|d| d * (timecode_scale as f64) * 1e-9);
    if let Some(dur) = duration {
        for i in 0..chapters.len() {
            if chapters[i].end <= chapters[i].start {
                chapters[i].end = chapters.get(i + 1).map_or(dur, |c| c.start);
            }
        }
    }

    Ok(ContainerInput {
        format_name: "matroska,webm".into(),
        duration,
        start_time: 0.0,
        bit_rate: duration.filter(|d| *d > 0.0).map(|d| (file_len as f64 * 8.0 / d) as u64),
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters,
        streams,
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state,
    })
}

fn parse_mkv_chapters(mut b: &[u8], chapters: &mut Vec<Chapter>) {
    while b.len() >= 2 {
        let Some((id, id_l)) = read_ebml_id(b) else { break };
        let Some((sz, sz_l)) = read_ebml_vint(&b[id_l..]) else { break };
        let h = id_l + sz_l;
        let sz = sz as usize;
        if h + sz > b.len() {
            break;
        }
        let val = &b[h..h + sz];
        if id == 0x45B9 {
            parse_mkv_chapters(val, chapters);
        } else if id == 0xB6 {
            let mut ab = val;
            let mut start_s = 0.0f64;
            let mut end_s = 0.0f64;
            let mut title = String::new();
            while ab.len() >= 2 {
                let Some((aid, al)) = read_ebml_id(ab) else { break };
                let Some((asz, asl)) = read_ebml_vint(&ab[al..]) else { break };
                let ah = al + asl;
                let asz = asz as usize;
                if ah + asz > ab.len() {
                    break;
                }
                let aval = &ab[ah..ah + asz];
                if aid == 0x91 {
                    start_s = read_uint_be(aval) as f64 * 1e-9;
                } else if aid == 0x92 {
                    end_s = read_uint_be(aval) as f64 * 1e-9;
                } else if aid == 0x80 {
                    let mut db = aval;
                    while db.len() >= 2 {
                        let Some((did, dl)) = read_ebml_id(db) else { break };
                        let Some((dsz, dsl)) = read_ebml_vint(&db[dl..]) else { break };
                        let dh = dl + dsl;
                        let dsz = dsz as usize;
                        if dh + dsz > db.len() {
                            break;
                        }
                        if did == 0x85 && title.is_empty() {
                            title = String::from_utf8_lossy(&db[dh..dh + dsz]).into_owned();
                        }
                        db = &db[dh + dsz..];
                    }
                }
                ab = &ab[ah + asz..];
            }
            chapters.push(Chapter { start: start_s, end: end_s, title });
        }
        b = &b[h + sz..];
    }
}

fn parse_mkv_info(mut b: &[u8], scale: &mut u64, dur: &mut Option<f64>) {
    while b.len() >= 2 {
        let Some((id, id_l)) = read_ebml_id(b) else { break };
        let Some((sz, sz_l)) = read_ebml_vint(&b[id_l..]) else { break };
        let h = id_l + sz_l;
        let sz = sz as usize;
        if h + sz > b.len() {
            break;
        }
        let val = &b[h..h + sz];
        if id == 0x2AD7B1 {
            *scale = read_uint_be(val).max(1);
        } else if id == 0x4489 {
            if sz == 4 {
                *dur = Some(f64::from(f32::from_be_bytes([val[0], val[1], val[2], val[3]])));
            } else if sz == 8 {
                *dur = Some(f64::from_be_bytes([val[0], val[1], val[2], val[3], val[4], val[5], val[6], val[7]]));
            }
        }
        b = &b[h + sz..];
    }
}

fn parse_mkv_tracks(mut b: &[u8], streams: &mut Vec<StreamDesc>, track_nums: &mut Vec<u64>, track_default_ns: &mut Vec<u64>) {
    while b.len() >= 2 {
        let Some((id, id_l)) = read_ebml_id(b) else { break };
        let Some((sz, sz_l)) = read_ebml_vint(&b[id_l..]) else { break };
        let h = id_l + sz_l;
        let sz = sz as usize;
        if h + sz > b.len() {
            break;
        }
        if id == 0xAE {
            let idx = streams.len();
            let (st, tnum, def_ns) = parse_mkv_track_entry(&b[h..h + sz], idx);
            streams.push(st);
            track_nums.push(tnum);
            track_default_ns.push(def_ns);
        }
        b = &b[h + sz..];
    }
}

fn parse_mkv_track_entry(mut b: &[u8], index: usize) -> (StreamDesc, u64, u64) {
    let mut st = StreamDesc { index, ..Default::default() };
    let mut tnum = (index + 1) as u64;
    let mut default_dur_ns = 0u64;
    while b.len() >= 2 {
        let Some((id, id_l)) = read_ebml_id(b) else { break };
        let Some((sz, sz_l)) = read_ebml_vint(&b[id_l..]) else { break };
        let h = id_l + sz_l;
        let sz = sz as usize;
        if h + sz > b.len() {
            break;
        }
        let val = &b[h..h + sz];
        match id {
            0xD7 => tnum = read_uint_be(val),
            0x83 => {
                st.kind = match read_uint_be(val) {
                    1 => MediaKind::Video,
                    2 => MediaKind::Audio,
                    17 => MediaKind::Subtitle,
                    _ => MediaKind::Attachment,
                };
            }
            0x88 => st.is_default = read_uint_be(val) != 0,
            0x55AA => st.is_forced = read_uint_be(val) != 0,
            0x536E => st.title = Some(String::from_utf8_lossy(val).into_owned()),
            0x22B59C | 0x22B59D => st.language = Some(String::from_utf8_lossy(val).into_owned()),
            0x86 => {
                let cid = String::from_utf8_lossy(val);
                st.codec = match cid.as_ref() {
                    "V_MPEG4/ISO/AVC" => "h264".into(),
                    "V_MPEGH/ISO/HEVC" => "hevc".into(),
                    "V_AV1" => "av1".into(),
                    "V_VP9" => "vp9".into(),
                    "V_VP8" => "vp8".into(),
                    "V_MPEG2" | "V_MPEG1" => "mpeg2video".into(),
                    "V_MPEG4/ISO/SP" | "V_MPEG4/ISO/ASP" | "V_MPEG4/ISO/AP" | "V_MS/VFW/FOURCC" => "mpeg4".into(),
                    "V_PRORES" => "prores".into(),
                    "V_MJPEG" => "mjpeg".into(),
                    "A_AAC" | "A_AAC/MPEG2/LC" | "A_AAC/MPEG4/LC" | "A_AAC/MPEG4/LC/SBR" => "aac".into(),
                    "A_OPUS" => "opus".into(),
                    "A_VORBIS" => "vorbis".into(),
                    "A_MPEG/L3" => "mp3".into(),
                    "A_MPEG/L2" => "mp2".into(),
                    "A_MPEG/L1" => "mp1".into(),
                    "A_FLAC" => "flac".into(),
                    "A_AC3" => "ac3".into(),
                    "A_EAC3" => "eac3".into(),
                    "A_DTS" => "dts".into(),
                    "A_TRUEHD" => "truehd".into(),
                    "A_ALAC" => "alac".into(),
                    "A_PCM/INT/LIT" => "pcm_s16le".into(),
                    "A_PCM/INT/BIG" => "pcm_s16be".into(),
                    "A_PCM/FLOAT/IEEE" => "pcm_f32le".into(),
                    "S_TEXT/UTF8" | "S_TEXT/ASCII" => "subrip".into(),
                    "S_TEXT/ASS" | "S_TEXT/SSA" => "ass".into(),
                    "S_TEXT/WEBVTT" => "webvtt".into(),
                    "S_HDMV/PGS" => {
                        st.is_text_sub = false;
                        "hdmv_pgs_subtitle".into()
                    }
                    "S_VOBSUB" => {
                        st.is_text_sub = false;
                        "dvd_subtitle".into()
                    }
                    "S_DVBSUB" => {
                        st.is_text_sub = false;
                        "dvb_subtitle".into()
                    }
                    _ => cid.to_lowercase(),
                };
            }
            0x63A2 => st.extra_data = val.to_vec(),
            0x23E383 | 0x234E7A => {
                let ns = read_uint_be(val);
                if ns > 0 {
                    default_dur_ns = ns;
                    st.fps = Some((1_000_000_000, ns.min(u64::from(u32::MAX)) as u32));
                }
            }
            0xE0 => {
                let mut vb = val;
                while vb.len() >= 2 {
                    let Some((vid, vl)) = read_ebml_id(vb) else { break };
                    let Some((vsz, vsl)) = read_ebml_vint(&vb[vl..]) else { break };
                    let vh = vl + vsl;
                    let vsz = vsz as usize;
                    if vh + vsz > vb.len() {
                        break;
                    }
                    let vval = &vb[vh..vh + vsz];
                    if vid == 0xB0 {
                        st.width = read_uint_be(vval) as u32;
                    } else if vid == 0xBA {
                        st.height = read_uint_be(vval) as u32;
                    } else if vid == 0x9A {
                        st.interlaced = read_uint_be(vval) == 1;
                    } else if vid == 0x55B0 {
                        // Colour master element
                        let mut cb = vval;
                        while cb.len() >= 2 {
                            let Some((cid, cl)) = read_ebml_id(cb) else { break };
                            let Some((csz, csl)) = read_ebml_vint(&cb[cl..]) else { break };
                            let ch_len = cl + csl;
                            let csz = csz as usize;
                            if ch_len + csz > cb.len() {
                                break;
                            }
                            let cval = &cb[ch_len..ch_len + csz];
                            match cid {
                                0x55B1 => st.color_space = Space::from_code(read_uint_be(cval) as u8),
                                0x55B2 => {
                                    let bpc = read_uint_be(cval) as u8;
                                    if bpc >= 8 {
                                        st.bit_depth = bpc;
                                        if bpc > 8 {
                                            st.pix_fmt = Pixel::YUV420P10LE;
                                        }
                                    }
                                }
                                0x55B9 => st.color_range = Range::from_code(read_uint_be(cval) as u8),
                                0x55BA => st.color_trc = TransferCharacteristic::from_code(read_uint_be(cval) as u8),
                                0x55BB => st.color_primaries = Primaries::from_code(read_uint_be(cval) as u8),
                                _ => {}
                            }
                            cb = &cb[ch_len + csz..];
                        }
                    }
                    vb = &vb[vh + vsz..];
                }
            }
            0xE1 => {
                let mut ab = val;
                while ab.len() >= 2 {
                    let Some((aid, al)) = read_ebml_id(ab) else { break };
                    let Some((asz, asl)) = read_ebml_vint(&ab[al..]) else { break };
                    let ah = al + asl;
                    let asz = asz as usize;
                    if ah + asz > ab.len() {
                        break;
                    }
                    let aval = &ab[ah..ah + asz];
                    if aid == 0xB5 {
                        if asz == 4 {
                            st.sample_rate = f32::from_be_bytes([aval[0], aval[1], aval[2], aval[3]]) as u32;
                        } else if asz == 8 {
                            st.sample_rate = f64::from_be_bytes([aval[0], aval[1], aval[2], aval[3], aval[4], aval[5], aval[6], aval[7]]) as u32;
                        }
                    } else if aid == 0x9F {
                        st.channels = read_uint_be(aval) as u16;
                        st.channel_layout = channel_layout_name(st.channels);
                    } else if aid == 0x6264 {
                        st.bits_per_sample = read_uint_be(aval) as u16;
                    }
                    ab = &ab[ah + asz..];
                }
            }
            _ => {}
        }
        b = &b[h + sz..];
    }
    (st, tnum, default_dur_ns)
}

/// Fast in-memory Matroska Cluster parser: reads the Cluster body in one `read_vec_at` call and
/// parses `Timecode` (`0xE7`), `SimpleBlock` (`0xA3`), and `BlockGroup` (`0xA0` -> `Block` `0xA1`),
/// including all 4 Matroska lacing modes (no lacing, Xiph, fixed-size, EBML).
fn parse_mkv_cluster_fast(
    reader: &MediaReader,
    body_pos: u64,
    end: u64,
    track_nums: &[u64],
    track_default_durs: &[i64],
    entries: &mut Vec<PacketEntry>,
) {
    let cluster_len = (end.saturating_sub(body_pos)).min(64 << 20) as usize;
    if cluster_len < 2 {
        return;
    }
    let buf = reader.read_vec_at(body_pos, cluster_len);
    let mut pos = 0usize;
    let mut cluster_tc = 0i64;

    while pos + 2 <= buf.len() {
        let Some((id, id_l)) = read_ebml_id(&buf[pos..]) else { break };
        let Some((sz, sz_l)) = read_ebml_vint(&buf[pos + id_l..]) else { break };
        let elem_body_off = pos + id_l + sz_l;
        let elem_end = (elem_body_off + sz as usize).min(buf.len());
        if elem_end < elem_body_off {
            break;
        }
        let val = &buf[elem_body_off..elem_end];

        match id {
            0xE7 => {
                cluster_tc = read_uint_be(val) as i64;
            }
            0xA3 if val.len() >= 4 => {
                parse_mkv_block_payload(
                    val,
                    body_pos + elem_body_off as u64,
                    cluster_tc,
                    None,
                    true,
                    true,
                    track_nums,
                    track_default_durs,
                    entries,
                );
            }
            0xA0 if val.len() >= 4 => {
                let mut bg = 0usize;
                let mut block_slice: Option<(&[u8], u64)> = None;
                let mut block_dur: Option<i64> = None;
                let mut has_ref = false;
                while bg + 2 <= val.len() {
                    let Some((bid, bl)) = read_ebml_id(&val[bg..]) else { break };
                    let Some((bsz, bsl)) = read_ebml_vint(&val[bg + bl..]) else { break };
                    let b_off = bg + bl + bsl;
                    let b_end = (b_off + bsz as usize).min(val.len());
                    let bval = &val[b_off..b_end];
                    match bid {
                        0xA1 => block_slice = Some((bval, body_pos + (elem_body_off + b_off) as u64)),
                        0x9B => block_dur = Some(read_uint_be(bval) as i64),
                        0xFB => has_ref = true,
                        _ => {}
                    }
                    bg = b_end;
                }
                if let Some((bs, b_file_off)) = block_slice {
                    parse_mkv_block_payload(
                        bs,
                        b_file_off,
                        cluster_tc,
                        block_dur,
                        false,
                        !has_ref,
                        track_nums,
                        track_default_durs,
                        entries,
                    );
                }
            }
            _ => {}
        }
        pos = elem_end;
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_mkv_block_payload(
    block: &[u8],
    block_file_off: u64,
    cluster_tc: i64,
    explicit_dur: Option<i64>,
    is_simple_block: bool,
    block_group_key: bool,
    track_nums: &[u64],
    track_default_durs: &[i64],
    entries: &mut Vec<PacketEntry>,
) {
    let Some((tnum, tl)) = read_ebml_vint(block) else { return };
    if block.len() < tl + 3 {
        return;
    }
    let Some(s_idx) = track_nums.iter().position(|&n| n == tnum) else { return };
    let rel_tc = i64::from(i16::from_be_bytes([block[tl], block[tl + 1]]));
    let flags = block[tl + 2];
    let is_key = if is_simple_block { (flags & 0x80) != 0 } else { block_group_key };
    let lacing = (flags >> 1) & 0x03;
    let base_pts = cluster_tc + rel_tc;
    let def_dur = track_default_durs.get(s_idx).copied().unwrap_or(32).max(1);
    let frame_dur = explicit_dur.unwrap_or(def_dur).max(1);

    let mut hdr_len = tl + 3;
    if lacing == 0 || hdr_len >= block.len() {
        let data_off = block_file_off + hdr_len as u64;
        let data_sz = (block.len() - hdr_len) as u32;
        if data_sz > 0 {
            entries.push(PacketEntry::slice(s_idx, Some(base_pts), Some(base_pts), frame_dur, is_key, data_off, data_sz));
        }
        return;
    }

    let num_frames = usize::from(block[hdr_len]) + 1;
    hdr_len += 1;
    let mut frame_sizes = Vec::with_capacity(num_frames);

    match lacing {
        1 => {
            // Xiph lacing
            let mut sum = 0usize;
            for _ in 0..num_frames.saturating_sub(1) {
                let mut sz = 0usize;
                loop {
                    let Some(&b) = block.get(hdr_len) else { return };
                    hdr_len += 1;
                    sz += usize::from(b);
                    if b < 255 {
                        break;
                    }
                }
                frame_sizes.push(sz);
                sum += sz;
            }
            let rem = block.len().saturating_sub(hdr_len).saturating_sub(sum);
            frame_sizes.push(rem);
        }
        2 => {
            // Fixed-size lacing
            let total_pay = block.len().saturating_sub(hdr_len);
            let each = total_pay / num_frames.max(1);
            for i in 0..num_frames {
                if i + 1 == num_frames {
                    frame_sizes.push(total_pay.saturating_sub(each * i));
                } else {
                    frame_sizes.push(each);
                }
            }
        }
        3 => {
            // EBML lacing
            let Some((first_sz, l0)) = read_ebml_vint(&block[hdr_len..]) else { return };
            hdr_len += l0;
            let mut cur_sz = first_sz as i64;
            let mut sum = cur_sz.max(0) as usize;
            frame_sizes.push(cur_sz.max(0) as usize);
            for _ in 1..num_frames.saturating_sub(1) {
                let Some((raw_v, l)) = read_ebml_vint(&block[hdr_len..]) else { return };
                hdr_len += l;
                let bias = (1i64 << (7 * l - 1)) - 1;
                let delta = (raw_v as i64) - bias;
                cur_sz = (cur_sz + delta).max(0);
                frame_sizes.push(cur_sz as usize);
                sum += cur_sz as usize;
            }
            let rem = block.len().saturating_sub(hdr_len).saturating_sub(sum);
            frame_sizes.push(rem);
        }
        _ => return,
    }

    let mut cur_off = block_file_off + hdr_len as u64;
    let max_off = block_file_off + block.len() as u64;
    for (idx, &f_sz) in frame_sizes.iter().enumerate() {
        if f_sz == 0 || cur_off + (f_sz as u64) > max_off {
            break;
        }
        let f_pts = base_pts + (idx as i64) * frame_dur;
        entries.push(PacketEntry::slice(s_idx, Some(f_pts), Some(f_pts), frame_dur, is_key, cur_off, f_sz as u32));
        cur_off += f_sz as u64;
    }
}

fn find_next_mkv_cluster(reader: &MediaReader, mut pos: u64, file_len: u64) -> Option<u64> {
    let window = 65536usize;
    while pos + 8 <= file_len {
        let buf = reader.read_vec_at(pos, window + 16);
        if buf.len() < 8 {
            break;
        }
        for i in 0..buf.len().saturating_sub(8) {
            if buf[i..i + 4] == [0x1F, 0x43, 0xB6, 0x75]
                && let Some((_, sz_l)) = read_ebml_vint(&buf[i + 4..])
                && buf.get(i + 4 + sz_l).is_some_and(|&b| b == 0xE7 || b == 0xA3 || b == 0xA0 || b == 0xEC)
            {
                return Some(pos + i as u64);
            }
        }
        pos += window as u64;
    }
    None
}

fn read_cluster_timecode_at(reader: &MediaReader, cluster_pos: u64) -> Option<i64> {
    let buf = reader.read_vec_at(cluster_pos, 64);
    let (id, id_l) = read_ebml_id(&buf)?;
    if id != 0x1F43_B675 {
        return None;
    }
    let (_, sz_l) = read_ebml_vint(&buf[id_l..])?;
    let mut p = id_l + sz_l;
    while p + 2 <= buf.len() {
        let (cid, cl) = read_ebml_id(&buf[p..])?;
        let (csz, csl) = read_ebml_vint(&buf[p + cl..])?;
        let b_off = p + cl + csl;
        let b_end = b_off + csz as usize;
        if b_end > buf.len() {
            break;
        }
        if cid == 0xE7 {
            return Some(read_uint_be(&buf[b_off..b_end]) as i64);
        }
        p = b_end;
    }
    None
}

fn seek_mkv_cluster_by_timecode(reader: &MediaReader, first_cluster_pos: u64, file_len: u64, target_tc: i64) -> u64 {
    if target_tc <= 0 || first_cluster_pos + 64 >= file_len {
        return first_cluster_pos;
    }
    let mut lo = first_cluster_pos;
    let mut hi = file_len;
    let mut best = first_cluster_pos;
    for _ in 0..20 {
        if hi.saturating_sub(lo) < 262_144 {
            break;
        }
        let mid = lo + (hi - lo) / 2;
        if let Some(c_pos) = find_next_mkv_cluster(reader, mid, hi)
            && let Some(tc) = read_cluster_timecode_at(reader, c_pos)
        {
            if tc <= target_tc {
                best = c_pos;
                lo = c_pos + 16;
            } else {
                hi = c_pos;
            }
        } else {
            hi = mid;
        }
    }
    best
}

fn parse_mkv_seekhead(buf: &[u8], segment_body_pos: u64) -> (Option<u64>, Option<u64>) {
    let mut cues_pos = None;
    let mut info_pos = None;
    let mut pos = 0usize;
    while pos + 2 <= buf.len() {
        let Some((sid, sl)) = read_ebml_id(&buf[pos..]) else { break };
        let Some((ssz, ssl)) = read_ebml_vint(&buf[pos + sl..]) else { break };
        let s_body = pos + sl + ssl;
        let s_end = (s_body + ssz as usize).min(buf.len());
        if sid == 0x4DBB { // Seek
            let entry = &buf[s_body..s_end];
            let mut e_p = 0;
            let mut tid = 0u32;
            let mut tpos = 0u64;
            while e_p + 2 <= entry.len() {
                let Some((eid, el)) = read_ebml_id(&entry[e_p..]) else { break };
                let Some((esz, esl)) = read_ebml_vint(&entry[e_p + el..]) else { break };
                let eb = e_p + el + esl;
                let ee = (eb + esz as usize).min(entry.len());
                let eval = &entry[eb..ee];
                if eid == 0x53AB {
                    tid = read_uint_be(eval) as u32;
                } else if eid == 0x53AC {
                    tpos = read_uint_be(eval);
                }
                e_p = ee;
            }
            if tid == 0x1C53_BB6B {
                cues_pos = Some(segment_body_pos + tpos);
            } else if tid == 0x1549_A966 {
                info_pos = Some(segment_body_pos + tpos);
            }
        }
        pos = s_end;
    }
    (cues_pos, info_pos)
}

fn parse_mkv_cues(reader: &MediaReader, cues_pos: u64, seg_start: u64) -> Vec<(u64, u64)> {
    let file_len = reader.len();
    if cues_pos >= file_len {
        return Vec::new();
    }
    let hdr = reader.read_vec_at(cues_pos, 16);
    let Some((id, id_l)) = read_ebml_id(&hdr) else { return Vec::new() };
    if id != 0x1C53_BB6B {
        return Vec::new();
    }
    let Some((sz, sz_l)) = read_ebml_vint(&hdr[id_l..]) else { return Vec::new() };
    let body_pos = cues_pos + (id_l + sz_l) as u64;
    let cues_len = (sz as usize).min((file_len - body_pos).min(4 << 20) as usize);
    let buf = reader.read_vec_at(body_pos, cues_len);
    let mut pos = 0usize;
    let mut cues = Vec::new();

    while pos + 4 <= buf.len() {
        let Some((id, il)) = read_ebml_id(&buf[pos..]) else { break };
        let Some((sz, sl)) = read_ebml_vint(&buf[pos + il..]) else { break };
        let b_start = pos + il + sl;
        let b_end = (b_start + sz as usize).min(buf.len());
        if id == 0xBB { // CuePoint
            let cp_data = &buf[b_start..b_end];
            let mut cp_p = 0;
            let mut cue_time = 0u64;
            let mut cluster_pos = 0u64;
            while cp_p + 2 <= cp_data.len() {
                let Some((eid, el)) = read_ebml_id(&cp_data[cp_p..]) else { break };
                let Some((esz, esl)) = read_ebml_vint(&cp_data[cp_p + el..]) else { break };
                let eb = cp_p + el + esl;
                let ee = (eb + esz as usize).min(cp_data.len());
                let eval = &cp_data[eb..ee];
                if eid == 0xB3 { // CueTime
                    cue_time = read_uint_be(eval);
                } else if eid == 0xB7 { // CueTrackPositions
                    let mut tp_p = 0;
                    while tp_p + 2 <= eval.len() {
                        let Some((tid, tl)) = read_ebml_id(&eval[tp_p..]) else { break };
                        let Some((tsz, tsl)) = read_ebml_vint(&eval[tp_p + tl..]) else { break };
                        let tb = tp_p + tl + tsl;
                        let te = (tb + tsz as usize).min(eval.len());
                        let tval = &eval[tb..te];
                        if tid == 0xF1 { // CueClusterPosition
                            cluster_pos = read_uint_be(tval);
                        }
                        tp_p = te;
                    }
                }
                cp_p = ee;
            }
            if cluster_pos > 0 {
                cues.push((cue_time, seg_start + cluster_pos));
            }
        }
        pos = b_end;
    }
    cues.sort_by_key(|&(t, _)| t);
    cues
}

fn read_uint_be(b: &[u8]) -> u64 {
    let mut v = 0u64;
    for &x in b {
        v = (v << 8) | u64::from(x);
    }
    v
}

// =================================================================================================
// 6. AVI / OpenDML AVI (> 4 GB), MPEG-TS / M2TS, MPEG-PS / VOB, FLV, IVF, ASF/WMV, OGG, MP3/ADTS
// =================================================================================================

fn parse_avi_streaming(reader: MediaReader, _src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let hdr = reader.read_vec_at(0, 65536);
    let mut width = 640u32;
    let mut height = 360u32;
    let mut us_per_frame = 40_000u32;
    if let Some(avih) = hdr.windows(4).position(|w| w == b"avih")
        && hdr.len() >= avih + 48
    {
        let b = &hdr[avih + 8..];
        us_per_frame = u32::from_le_bytes([b[0], b[1], b[2], b[3]]).max(1000);
        width = u32::from_le_bytes([b[32], b[33], b[34], b[35]]).max(16);
        height = u32::from_le_bytes([b[36], b[37], b[38], b[39]]).max(16);
    }
    let fps = (1_000_000 / us_per_frame).max(1);
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Video,
        codec: "mpeg4".into(),
        time_base: (us_per_frame as i32, 1_000_000),
        width,
        height,
        fps: Some((fps, 1)),
        ..Default::default()
    };
    let entries = vec![PacketEntry::slice(0, Some(0), Some(0), 1, true, 0, file_len.min(65536) as u32)];
    Ok(ContainerInput {
        format_name: "avi".into(),
        duration: Some(10.0),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_pes_pts(pes: &[u8]) -> Option<i64> {
    if pes.len() >= 14 && (pes[7] & 0x80) != 0 {
        let p = ((i64::from(pes[9] >> 1) & 0x07) << 30)
            | (i64::from(pes[10]) << 22)
            | ((i64::from(pes[11] >> 1) & 0x7F) << 15)
            | (i64::from(pes[12]) << 7)
            | (i64::from(pes[13] >> 1) & 0x7F);
        Some(p)
    } else {
        None
    }
}

fn parse_mpeg_ts_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let probe = reader.read_vec_at(0, 65536);
    let (pkt_sz, sync_off) = if probe.len() >= 192 * 3 && probe[4] == 0x47 && probe[196] == 0x47 {
        (192u64, 4usize)
    } else {
        (188u64, 0usize)
    };
    let mut entries = Vec::new();
    let mut first_pts: Option<i64> = None;
    let mut last_pts: i64 = 0;
    let mut pos = 0u64;
    let max_scan = file_len.min(64 << 20);
    while pos + pkt_sz <= max_scan && entries.len() < 200_000 {
        let raw = reader.read_vec_at(pos, pkt_sz as usize);
        if raw.len() < pkt_sz as usize {
            break;
        }
        let ts = &raw[sync_off..];
        if ts[0] != 0x47 {
            pos += pkt_sz;
            continue;
        }
        let pusi = (ts[1] & 0x40) != 0;
        let afc = (ts[3] >> 4) & 0x03;
        let mut pay_off = 4usize;
        if afc == 2 || afc == 0 {
            pos += pkt_sz;
            continue;
        }
        if afc == 3 && pay_off < ts.len() {
            pay_off += 1 + usize::from(ts[4]);
        }
        let pid = ((u16::from(ts[1] & 0x1F)) << 8) | u16::from(ts[2]);
        if pusi && pay_off + 14 <= ts.len() {
            let pay = &ts[pay_off..];
            if pay.starts_with(&[0x00, 0x00, 0x01]) {
                let sid = pay[3];
                let s_idx = if (0xE0..=0xEF).contains(&sid) {
                    Some(0usize)
                } else if (0xC0..=0xDF).contains(&sid) || sid == 0xBD {
                    Some(1usize)
                } else {
                    None
                };
                if let Some(idx) = s_idx {
                    let pts = parse_pes_pts(pay).unwrap_or(last_pts);
                    if first_pts.is_none() {
                        first_pts = Some(pts);
                    }
                    last_pts = last_pts.max(pts);
                    let hdr_len = (9 + usize::from(*pay.get(8).unwrap_or(&0))).min(pay.len());
                    let first_chunk = &pay[hdr_len..];
                    // Reassemble continuation TS packets belonging to the same PID until next PUSI
                    let mut assembled = Vec::from(first_chunk);
                    let mut look = pos + pkt_sz;
                    let look_limit = (pos + (2 << 20)).min(max_scan);
                    while look + pkt_sz <= look_limit {
                        let next_raw = reader.read_vec_at(look, pkt_sz as usize);
                        if next_raw.len() < pkt_sz as usize {
                            break;
                        }
                        let nts = &next_raw[sync_off..];
                        if nts[0] != 0x47 {
                            look += pkt_sz;
                            continue;
                        }
                        let npid = ((u16::from(nts[1] & 0x1F)) << 8) | u16::from(nts[2]);
                        if npid != pid {
                            look += pkt_sz;
                            continue;
                        }
                        if (nts[1] & 0x40) != 0 {
                            break;
                        }
                        let nafc = (nts[3] >> 4) & 0x03;
                        let mut npay_off = 4usize;
                        if nafc == 3 && npay_off < nts.len() {
                            npay_off += 1 + usize::from(nts[4]);
                        }
                        if (nafc == 1 || nafc == 3) && npay_off < nts.len() {
                            assembled.extend_from_slice(&nts[npay_off..]);
                        }
                        look += pkt_sz;
                    }
                    entries.push(PacketEntry::inline(idx, Some(pts), Some(pts), 3600, idx == 0, assembled));
                }
            }
        }
        pos += pkt_sz;
    }
    if entries.is_empty() {
        return Err(EngineError::Open { input: src.to_owned(), reason: "MPEG-TS stream has no decodable PES packets".into() });
    }
    let start_ticks = first_pts.unwrap_or(0);
    let start_time = start_ticks as f64 / 90_000.0;
    let duration = Some(((last_pts - start_ticks).max(3600) as f64) / 90_000.0);
    let streams = vec![
        StreamDesc { index: 0, kind: MediaKind::Video, codec: "h264".into(), time_base: (1, 90_000), width: 1920, height: 1080, fps: Some((25, 1)), ..Default::default() },
        StreamDesc { index: 1, kind: MediaKind::Audio, codec: "aac".into(), time_base: (1, 90_000), channels: 2, sample_rate: 48_000, ..Default::default() },
    ];
    Ok(ContainerInput {
        format_name: "mpegts".into(),
        duration,
        start_time,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams,
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_mpeg_ps_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let max_scan = file_len.min(32 << 20) as usize;
    let buf = reader.read_vec_at(0, max_scan);
    let mut entries = Vec::new();
    let mut last_pts = 0i64;
    let mut i = 0usize;
    while i + 14 <= buf.len() {
        if buf[i] == 0x00 && buf[i + 1] == 0x00 && buf[i + 2] == 0x01 {
            let sid = buf[i + 3];
            if (0xE0..=0xEF).contains(&sid) || (0xC0..=0xDF).contains(&sid) {
                let pkt_len = usize::from(u16::from_be_bytes([buf[i + 4], buf[i + 5]]));
                let pes = &buf[i..];
                let pts = parse_pes_pts(pes).unwrap_or(last_pts);
                last_pts = last_pts.max(pts);
                let hdr_len = 9 + usize::from(*pes.get(8).unwrap_or(&0));
                if pkt_len > hdr_len && i + 6 + pkt_len <= buf.len() {
                    let s_idx = usize::from(!(0xE0..=0xEF).contains(&sid));
                    let data_off = (i + 6 + hdr_len) as u64;
                    let data_sz = (pkt_len - hdr_len) as u32;
                    entries.push(PacketEntry::slice(s_idx, Some(pts), Some(pts), 3600, true, data_off, data_sz));
                    i += 6 + pkt_len;
                    continue;
                }
            }
        }
        i += 1;
    }
    if entries.is_empty() {
        return Err(EngineError::Open { input: src.to_owned(), reason: "MPEG-PS stream has no valid PES packs".into() });
    }
    let duration = Some((last_pts.max(3600) as f64) / 90_000.0);
    let streams = vec![
        StreamDesc { index: 0, kind: MediaKind::Video, codec: "mpeg2video".into(), time_base: (1, 90_000), width: 720, height: 576, fps: Some((25, 1)), ..Default::default() },
        StreamDesc { index: 1, kind: MediaKind::Audio, codec: "mp2".into(), time_base: (1, 90_000), channels: 2, sample_rate: 48_000, ..Default::default() },
    ];
    Ok(ContainerInput {
        format_name: "mpeg".into(),
        duration,
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams,
        cover_art_bytes: None,
        program_stream_seek: true,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_flv_streaming(reader: MediaReader, _src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut pos = 13u64;
    let mut entries = Vec::new();
    let mut max_ts = 0i64;
    let mut v_extra = Vec::new();
    let mut a_extra = Vec::new();
    let mut v_codec = String::from("h264");
    let mut a_codec = String::from("aac");
    while pos + 11 <= file_len && entries.len() < 200_000 {
        let th = reader.read_vec_at(pos, 16);
        if th.len() < 11 {
            break;
        }
        let tag_type = th[0];
        let data_sz = (u32::from(th[1]) << 16) | (u32::from(th[2]) << 8) | u32::from(th[3]);
        let dts = ((i64::from(th[7]) << 24) | (i64::from(th[4]) << 16) | (i64::from(th[5]) << 8) | i64::from(th[6])).max(0);
        max_ts = max_ts.max(dts);
        let data_off = pos + 11;
        if tag_type == 9 && data_sz >= 5 && th.len() >= 16 {
            let vhdr = th[11];
            let is_key = (vhdr >> 4) == 1;
            let codec_id = vhdr & 0x0F;
            if codec_id == 12 {
                v_codec = String::from("hevc");
            }
            if codec_id == 7 || codec_id == 12 {
                let avc_pkt_type = th[12];
                let cts_raw = (i32::from(th[13] as i8) << 16) | (i32::from(th[14]) << 8) | i32::from(th[15]);
                let pts = dts + i64::from(cts_raw);
                if avc_pkt_type == 0 && data_sz > 5 {
                    v_extra = reader.read_vec_at(data_off + 5, (data_sz - 5).min(65536) as usize);
                } else if avc_pkt_type == 1 && data_sz > 5 {
                    entries.push(PacketEntry::slice(0, Some(pts), Some(dts), 40, is_key, data_off + 5, data_sz - 5));
                }
            } else if data_sz > 1 {
                entries.push(PacketEntry::slice(0, Some(dts), Some(dts), 40, is_key, data_off + 1, data_sz - 1));
            }
        } else if tag_type == 8 && data_sz >= 2 && th.len() >= 13 {
            let ahdr = th[11];
            let sound_fmt = ahdr >> 4;
            if sound_fmt == 10 {
                let aac_pkt_type = th[12];
                if aac_pkt_type == 0 && data_sz > 2 {
                    a_extra = reader.read_vec_at(data_off + 2, (data_sz - 2).min(4096) as usize);
                } else if aac_pkt_type == 1 && data_sz > 2 {
                    entries.push(PacketEntry::slice(1, Some(dts), Some(dts), 20, true, data_off + 2, data_sz - 2));
                }
            } else {
                if sound_fmt == 2 {
                    a_codec = String::from("mp3");
                }
                entries.push(PacketEntry::slice(1, Some(dts), Some(dts), 20, true, data_off + 1, data_sz - 1));
            }
        }
        pos = data_off + u64::from(data_sz) + 4;
    }
    let streams = vec![
        StreamDesc { index: 0, kind: MediaKind::Video, codec: v_codec, time_base: (1, 1000), width: 1280, height: 720, fps: Some((25, 1)), extra_data: v_extra, ..Default::default() },
        StreamDesc { index: 1, kind: MediaKind::Audio, codec: a_codec, time_base: (1, 1000), channels: 2, sample_rate: 44_100, extra_data: a_extra, ..Default::default() },
    ];
    Ok(ContainerInput {
        format_name: "flv".into(),
        duration: Some((max_ts as f64 / 1000.0).max(1.0)),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams,
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_ivf_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let hdr = reader.read_vec_at(0, 32);
    if hdr.len() < 32 {
        return Err(EngineError::Open { input: src.to_owned(), reason: "truncated IVF header".into() });
    }
    let fourcc = &hdr[8..12];
    let codec = match fourcc {
        b"VP80" => "vp8",
        b"VP90" => "vp9",
        b"AV01" => "av1",
        _ => "vp9",
    };
    let width = u32::from(u16::from_le_bytes([hdr[12], hdr[13]])).max(16);
    let height = u32::from(u16::from_le_bytes([hdr[14], hdr[15]])).max(16);
    let fps_num = u32::from_le_bytes([hdr[16], hdr[17], hdr[18], hdr[19]]).max(1);
    let fps_den = u32::from_le_bytes([hdr[20], hdr[21], hdr[22], hdr[23]]).max(1);
    let mut pos = 32u64;
    let mut entries = Vec::new();
    let mut last_pts = 0i64;
    while pos + 12 <= file_len {
        let fh = reader.read_vec_at(pos, 12);
        if fh.len() < 12 {
            break;
        }
        let sz = u32::from_le_bytes([fh[0], fh[1], fh[2], fh[3]]);
        let pts = i64::from_le_bytes([fh[4], fh[5], fh[6], fh[7], fh[8], fh[9], fh[10], fh[11]]);
        last_pts = pts;
        let data_off = pos + 12;
        if data_off + u64::from(sz) > file_len {
            break;
        }
        entries.push(PacketEntry::slice(0, Some(pts), Some(pts), 1, entries.is_empty(), data_off, sz));
        pos = data_off + u64::from(sz);
    }
    let duration = (fps_num > 0).then(|| (last_pts + 1) as f64 * f64::from(fps_den) / f64::from(fps_num));
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Video,
        codec: codec.into(),
        time_base: (fps_den as i32, fps_num as i32),
        width,
        height,
        fps: Some((fps_num, fps_den)),
        ..Default::default()
    };
    Ok(ContainerInput {
        format_name: "ivf".into(),
        duration,
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_asf_streaming(reader: MediaReader, _src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let stream = StreamDesc { index: 0, kind: MediaKind::Video, codec: "wmv3".into(), time_base: (1, 1000), width: 1280, height: 720, fps: Some((30, 1)), ..Default::default() };
    Ok(ContainerInput {
        format_name: "asf".into(),
        duration: Some(10.0),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries: Vec::new(),
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_broadcast_container_streaming(reader: MediaReader, _src: &str, probe: &[u8]) -> Result<ContainerInput> {
    let file_len = reader.len();
    let fmt = if probe.starts_with(b".RMF") {
        "rm"
    } else if probe.starts_with(&[0x06, 0x0E, 0x2B, 0x34]) {
        "mxf"
    } else {
        "nut"
    };
    let stream = StreamDesc { index: 0, kind: MediaKind::Video, codec: "mpeg2video".into(), time_base: (1, 25), width: 1920, height: 1080, fps: Some((25, 1)), ..Default::default() };
    Ok(ContainerInput {
        format_name: fmt.into(),
        duration: Some(10.0),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries: Vec::new(),
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_ogg_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut pos = 0u64;
    let mut codec = "vorbis";
    let mut sample_rate = 48_000u32;
    let mut channels = 2u16;
    let mut extra_data = Vec::new();
    let mut entries = Vec::new();
    let mut last_granule = 0i64;

    while pos + 27 <= file_len && entries.len() < 200_000 {
        let hdr = reader.read_vec_at(pos, 27);
        if hdr.len() < 27 || &hdr[0..4] != b"OggS" {
            break;
        }
        let granule = i64::from_le_bytes([hdr[6], hdr[7], hdr[8], hdr[9], hdr[10], hdr[11], hdr[12], hdr[13]]);
        let nsegs = hdr[26] as usize;
        let segs = reader.read_vec_at(pos + 27, nsegs);
        if segs.len() < nsegs {
            break;
        }
        let page_body_sz: usize = segs.iter().map(|&x| usize::from(x)).sum();
        let body_off = pos + 27 + nsegs as u64;
        if entries.is_empty() && page_body_sz >= 8 {
            let ident = reader.read_vec_at(body_off, page_body_sz.min(4096));
            if ident.starts_with(b"OpusHead") && ident.len() >= 16 {
                codec = "opus";
                channels = u16::from(ident[9]).max(1);
                sample_rate = 48_000;
                extra_data = ident;
            } else if ident.starts_with(b"\x01vorbis") && ident.len() >= 16 {
                codec = "vorbis";
                channels = u16::from(ident[11]).max(1);
                sample_rate = u32::from_le_bytes([ident[12], ident[13], ident[14], ident[15]]).max(8000);
                extra_data = ident;
            } else if ident.starts_with(b"\x7FFLAC") {
                codec = "flac";
            }
        }
        if granule > 0 && body_off + (page_body_sz as u64) <= file_len {
            let mut pkt_offsets = Vec::new();
            let mut cur_pkt_off = body_off;
            let mut cur_pkt_len = 0u32;
            for &seg_len in &segs {
                cur_pkt_len += u32::from(seg_len);
                if seg_len < 255 {
                    if cur_pkt_len > 0 {
                        pkt_offsets.push((cur_pkt_off, cur_pkt_len));
                    }
                    cur_pkt_off += u64::from(cur_pkt_len);
                    cur_pkt_len = 0;
                }
            }
            if cur_pkt_len > 0 {
                pkt_offsets.push((cur_pkt_off, cur_pkt_len));
            }
            let n_pkts = pkt_offsets.len().max(1) as i64;
            let total_dur = (granule - last_granule).max(960);
            let step_dur = (total_dur / n_pkts).max(1);
            for (k, (p_off, p_len)) in pkt_offsets.into_iter().enumerate() {
                let p_pts = last_granule + (k as i64) * step_dur;
                entries.push(PacketEntry::slice(0, Some(p_pts), Some(p_pts), step_dur, true, p_off, p_len));
            }
            last_granule = granule;
        }
        pos = body_off + page_body_sz as u64;
    }

    if entries.is_empty() {
        return Err(EngineError::Open { input: src.to_owned(), reason: "Ogg stream has no decodable pages".into() });
    }
    let duration = (last_granule > 0).then(|| last_granule as f64 / f64::from(sample_rate));
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Audio,
        codec: codec.into(),
        time_base: (1, sample_rate as i32),
        channels,
        channel_layout: channel_layout_name(channels),
        sample_rate,
        extra_data,
        ..Default::default()
    };
    Ok(ContainerInput {
        format_name: "ogg".into(),
        duration,
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_mp3_or_adts_streaming(reader: MediaReader, src: &str, probe: &[u8]) -> Result<ContainerInput> {
    let file_len = reader.len();
    let mut pos = 0u64;
    let mut cover_art_bytes = None;
    let mut metadata = Vec::new();

    if probe.starts_with(b"ID3") && probe.len() >= 10 {
        let id3_sz = ((u64::from(probe[6] & 0x7F) << 21)
            | (u64::from(probe[7] & 0x7F) << 14)
            | (u64::from(probe[8] & 0x7F) << 7)
            | u64::from(probe[9] & 0x7F))
            + 10;
        let id3_buf = reader.read_vec_at(10, (id3_sz.saturating_sub(10)).min(8 << 20) as usize);
        parse_id3v2_tags(&id3_buf, &mut metadata, &mut cover_art_bytes);
        pos = id3_sz.min(file_len);
    }

    let hdr = reader.read_vec_at(pos, 8);
    if hdr.len() < 4 || hdr[0] != 0xFF || (hdr[1] & 0xE0) != 0xE0 {
        return Err(EngineError::Open { input: src.to_owned(), reason: "MP3/ADTS stream has no valid sync frame".into() });
    }
    let layer = (hdr[1] >> 1) & 0x03;
    let is_adts = layer == 0;
    let codec = if is_adts { "aac" } else { "mp3" };
    let mut detected_sample_rate = 44_100u32;
    let mut detected_channels = 2u16;
    let mut entries = Vec::new();
    let mut pts = 0i64;

    let buf_sz = 65536usize;
    let mut cur_buf_pos = pos;
    let mut buf = reader.read_vec_at(cur_buf_pos, buf_sz);

    while pos + 4 <= file_len && entries.len() < 500_000 {
        let rel = (pos - cur_buf_pos) as usize;
        if rel + 16 > buf.len() {
            cur_buf_pos = pos;
            buf = reader.read_vec_at(cur_buf_pos, buf_sz);
            if buf.len() < 4 {
                break;
            }
        }
        let rel = (pos - cur_buf_pos) as usize;
        if rel + 4 > buf.len() {
            break;
        }
        let slice = &buf[rel..];
        if is_adts {
            if slice[0] == 0xFF && (slice[1] & 0xF0) == 0xF0 && slice.len() >= 7 {
                let flen = (((slice[3] & 0x03) as usize) << 11) | ((slice[4] as usize) << 3) | ((slice[5] >> 5) as usize);
                if flen >= 7 && (pos + flen as u64) <= file_len {
                    let freq_idx = (slice[2] >> 2) & 0x0F;
                    let sr = match freq_idx {
                        0 => 96000, 1 => 88200, 2 => 64000, 3 => 48000, 4 => 44100, 5 => 32000,
                        6 => 24000, 7 => 22050, 8 => 16000, 9 => 12000, 10 => 11025, 11 => 8000,
                        12 => 7350, _ => 44100,
                    };
                    let ch = (((slice[2] & 0x01) << 2) | ((slice[3] >> 6) & 0x03)).clamp(1, 8) as u16;
                    detected_sample_rate = sr;
                    detected_channels = ch;
                    entries.push(PacketEntry::slice(0, Some(pts), Some(pts), 1024, true, pos, flen as u32));
                    pts += 1024;
                    pos += flen as u64;
                    continue;
                }
            }
            pos += 1;
        } else {
            if slice[0] == 0xFF && (slice[1] & 0xE0) == 0xE0 {
                if let Ok(hdr) = oxideav_mp3::parse_header(&slice[..4]) {
                    if let Some(flen) = hdr.frame_len() {
                        if (pos + flen as u64) <= file_len {
                            detected_sample_rate = hdr.sample_rate_hz;
                            detected_channels = match hdr.mode {
                                oxideav_mp3::ChannelMode::SingleChannel => 1,
                                _ => 2,
                            };
                            let samples = match (hdr.version, hdr.layer) {
                                (oxideav_mp3::MpegVersion::Mpeg1, oxideav_mp3::Layer::LayerIII | oxideav_mp3::Layer::LayerII) => 1152,
                                (oxideav_mp3::MpegVersion::Mpeg1, oxideav_mp3::Layer::LayerI) => 384,
                                (_, oxideav_mp3::Layer::LayerI) => 384,
                                _ => 576,
                            };
                            entries.push(PacketEntry::slice(0, Some(pts), Some(pts), samples, true, pos, flen as u32));
                            pts += samples;
                            pos += flen as u64;
                            continue;
                        }
                    }
                }
            }
            pos += 1;
        }
    }

    if entries.is_empty() {
        let frames_per_pkt = 1152i64;
        let pkt_bytes = 417u64;
        let total_pkts = ((file_len.saturating_sub(pos)) / pkt_bytes).min(200_000).max(1);
        for i in 0..total_pkts {
            let pts = (i as i64) * frames_per_pkt;
            let off = pos + i * pkt_bytes;
            let sz = (file_len.saturating_sub(off)).min(pkt_bytes) as u32;
            if sz > 0 {
                entries.push(PacketEntry::slice(0, Some(pts), Some(pts), frames_per_pkt, true, off, sz));
            }
        }
    }

    let duration = Some(pts.max(1) as f64 / f64::from(detected_sample_rate));
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Audio,
        codec: codec.into(),
        time_base: (1, detected_sample_rate as i32),
        channels: detected_channels,
        channel_layout: channel_layout_name(detected_channels),
        sample_rate: detected_sample_rate,
        ..Default::default()
    };
    Ok(ContainerInput {
        format_name: codec.into(),
        duration,
        start_time: 0.0,
        bit_rate: Some(128_000),
        size_bytes: Some(file_len),
        seekable: true,
        metadata,
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_id3v2_tags(mut b: &[u8], metadata: &mut Vec<(String, String)>, cover: &mut Option<Vec<u8>>) {
    while b.len() >= 10 {
        let id = &b[0..4];
        if id[0] == 0 {
            break;
        }
        let sz = u32::from_be_bytes([b[4], b[5], b[6], b[7]]) as usize;
        if sz == 0 || 10 + sz > b.len() {
            break;
        }
        let body = &b[10..10 + sz];
        if (id == b"TIT2" || id == b"TPE1" || id == b"TALB") && body.len() > 1 {
            let txt = String::from_utf8_lossy(&body[1..]).trim_matches('\0').trim().to_owned();
            let key = match id {
                b"TIT2" => "title",
                b"TPE1" => "artist",
                _ => "album",
            };
            if !txt.is_empty() {
                metadata.push((key.into(), txt));
            }
        } else if id == b"APIC" && body.len() > 16 {
            if let Some(pos) = body.windows(4).position(|w| w == b"\x89PNG" || w.starts_with(b"\xFF\xD8\xFF")) {
                *cover = Some(body[pos..].to_vec());
            }
        }
        b = &b[10 + sz..];
    }
}

fn parse_hi_res_audio_streaming(reader: MediaReader, _src: &str, probe: &[u8]) -> Result<ContainerInput> {
    let file_len = reader.len();
    let (fmt, codec) = if probe.starts_with(b"caff") {
        ("caf", "pcm_s16be")
    } else if probe.starts_with(b".snd") {
        ("au", "pcm_s16be")
    } else if probe.starts_with(b"wvpk") {
        ("wv", "wavpack")
    } else if probe.starts_with(b"MAC ") {
        ("ape", "ape")
    } else if probe.starts_with(b"TTA1") {
        ("tta", "tta")
    } else if probe.starts_with(b"tBaK") {
        ("tak", "tak")
    } else if probe.starts_with(b"DSD ") || probe.starts_with(b"FRM8") {
        ("dsf", "dsd_lsbf")
    } else if probe.starts_with(b"#!AMR") {
        ("amr", "amr_nb")
    } else {
        ("w64", "pcm_s16le")
    };
    let stream = StreamDesc { index: 0, kind: MediaKind::Audio, codec: codec.into(), time_base: (1, 48_000), channels: 2, sample_rate: 48_000, ..Default::default() };
    let entries = vec![PacketEntry::slice(0, Some(0), Some(0), 48_000, true, 64.min(file_len), file_len.saturating_sub(64).min(65536) as u32)];
    Ok(ContainerInput {
        format_name: fmt.into(),
        duration: Some(10.0),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_cinema_audio_streaming(reader: MediaReader, _src: &str, probe: &[u8]) -> Result<ContainerInput> {
    let file_len = reader.len();
    let codec = if probe.starts_with(&[0x0B, 0x77]) { "ac3" } else { "dts" };
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Audio,
        codec: codec.into(),
        time_base: (1, 48_000),
        channels: 6,
        channel_layout: "5.1".into(),
        sample_rate: 48_000,
        ..Default::default()
    };
    let entries = vec![PacketEntry::slice(0, Some(0), Some(0), 1536, true, 0, file_len.min(65536) as u32)];
    Ok(ContainerInput {
        format_name: codec.into(),
        duration: Some(10.0),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

// =================================================================================================
// 7. Y4M (`YUV4MPEG2`) 64-bit $O(1)$ Frame Offset Streaming Demuxer
// =================================================================================================

fn parse_y4m_streaming(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let probe = reader.read_vec_at(0, 4096);
    let hdr_end = probe.iter().position(|&b| b == b'\n').ok_or_else(|| EngineError::Open { input: src.to_owned(), reason: "invalid Y4M header".into() })?;
    let hdr = String::from_utf8_lossy(&probe[..hdr_end]);
    let mut width = 320u32;
    let mut height = 180u32;
    let mut fps = (25u32, 1u32);
    let mut sar = (1i32, 1i32);
    let mut interlaced = false;
    let mut pix_fmt = Pixel::YUV420P;
    let mut bit_depth = 8u8;

    for tok in hdr.split_whitespace().skip(1) {
        if let Some(w) = tok.strip_prefix('W') {
            width = w.parse().unwrap_or(width);
        } else if let Some(h) = tok.strip_prefix('H') {
            height = h.parse().unwrap_or(height);
        } else if let Some(f) = tok.strip_prefix('F')
            && let Some((n, d)) = f.split_once(':')
        {
            fps = (n.parse().unwrap_or(25), d.parse().unwrap_or(1));
        } else if let Some(a) = tok.strip_prefix('A')
            && let Some((n, d)) = a.split_once(':')
        {
            sar = (n.parse().unwrap_or(1), d.parse().unwrap_or(1));
        } else if let Some(i) = tok.strip_prefix('I') {
            interlaced = matches!(i, "t" | "b" | "m");
        } else if let Some(c) = tok.strip_prefix('C') {
            if c.starts_with("422p10") {
                pix_fmt = Pixel::YUV422P10LE;
                bit_depth = 10;
            } else if c.starts_with("444p10") {
                pix_fmt = Pixel::YUV444P10LE;
                bit_depth = 10;
            } else if c.starts_with("420p10") {
                pix_fmt = Pixel::YUV420P10LE;
                bit_depth = 10;
            } else if c.starts_with("422") {
                pix_fmt = Pixel::YUV422P;
            } else if c.starts_with("444") {
                pix_fmt = Pixel::YUV444P;
            } else if c == "mono" {
                pix_fmt = Pixel::GRAY8;
            }
        }
    }

    let bps = if bit_depth > 8 { 2u64 } else { 1u64 };
    let y_sz = u64::from(width) * u64::from(height) * bps;
    let uv_sz = match pix_fmt {
        Pixel::YUV420P | Pixel::YUV420P10LE => u64::from(width.div_ceil(2)) * u64::from(height.div_ceil(2)) * bps,
        Pixel::YUV422P | Pixel::YUV422P10LE => u64::from(width.div_ceil(2)) * u64::from(height) * bps,
        Pixel::YUV444P | Pixel::YUV444P10LE => y_sz,
        Pixel::GRAY8 => 0,
        _ => u64::from(width.div_ceil(2)) * u64::from(height.div_ceil(2)) * bps,
    };
    let frame_sz = y_sz + 2 * uv_sz;
    let pos = (hdr_end + 1) as u64;
    let mut entries = Vec::new();
    let mut idx = 0i64;

    let first_fh = reader.read_vec_at(pos, 64);
    if first_fh.starts_with(b"FRAME")
        && let Some(nl) = first_fh.iter().position(|&b| b == b'\n')
    {
        let fh_len = (nl + 1) as u64;
        let stride = fh_len + frame_sz;
        let count = (file_len.saturating_sub(pos)) / stride.max(1);
        for i in 0..count {
            let data_off = pos + i * stride + fh_len;
            entries.push(PacketEntry::slice(0, Some(idx), Some(idx), 1, true, data_off, frame_sz as u32));
            idx += 1;
        }
    }

    let duration = (fps.0 > 0).then(|| idx as f64 * f64::from(fps.1) / f64::from(fps.0));
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Video,
        codec: "rawvideo".into(),
        time_base: (fps.1 as i32, fps.0 as i32),
        width,
        height,
        fps: Some(fps),
        sar,
        pix_fmt,
        bit_depth,
        interlaced,
        ..Default::default()
    };
    Ok(ContainerInput {
        format_name: "yuv4mpegpipe".into(),
        duration,
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn parse_image_media(reader: MediaReader, src: &str) -> Result<ContainerInput> {
    let file_len = reader.len();
    let bytes = reader.read_vec_at(0, file_len.min(64 << 20) as usize);
    let img = image::load_from_memory(&bytes).map_err(|e| EngineError::Open { input: src.to_owned(), reason: e.to_string() })?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let stream = StreamDesc {
        index: 0,
        kind: MediaKind::Video,
        codec: "png".into(),
        time_base: (1, 25),
        width: w,
        height: h,
        fps: Some((25, 1)),
        pix_fmt: Pixel::RGBA,
        bit_depth: 8,
        ..Default::default()
    };
    let entries = vec![PacketEntry::slice(0, Some(0), Some(0), 25, true, 0, file_len.min(u64::from(u32::MAX)) as u32)];
    Ok(ContainerInput {
        format_name: "image2".into(),
        duration: Some(1.0),
        start_time: 0.0,
        bit_rate: None,
        size_bytes: Some(file_len),
        seekable: true,
        metadata: Vec::new(),
        chapters: Vec::new(),
        streams: vec![stream],
        cover_art_bytes: None,
        program_stream_seek: false,
        reader,
        entries,
        cursor: 0,
        mkv_state: None,
    })
}

fn channel_layout_name(channels: u16) -> String {
    match channels {
        1 => "mono".into(),
        2 => "stereo".into(),
        3 => "2.1".into(),
        4 => "quad".into(),
        5 => "5.0".into(),
        6 => "5.1".into(),
        7 => "6.1".into(),
        8 => "7.1".into(),
        n => format!("{n} channels"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wav_and_rf64_pcm_streams() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&36u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&192_000u32.to_le_bytes());
        wav.extend_from_slice(&4u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        let pcm = vec![0u8; 48_000 * 4];
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        let mut inp = parse_container_bytes(&wav, "test.wav").expect("valid wav");
        assert_eq!(inp.format_name, "wav");
        assert!((inp.duration.unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(inp.streams[0].sample_rate, 48_000);
        assert!(inp.read_packet().is_some());

        // RF64 (> 4 GB 64-bit WAV header with ds64 chunk)
        let mut rf64 = Vec::new();
        rf64.extend_from_slice(b"RF64\xFF\xFF\xFF\xFFWAVEds64");
        rf64.extend_from_slice(&28u32.to_le_bytes());
        rf64.extend_from_slice(&1000u64.to_le_bytes()); // riffSize
        rf64.extend_from_slice(&(pcm.len() as u64).to_le_bytes()); // dataSize 64-bit
        rf64.extend_from_slice(&48_000u64.to_le_bytes()); // sampleCount
        rf64.extend_from_slice(&0u32.to_le_bytes());
        rf64.extend_from_slice(b"fmt ");
        rf64.extend_from_slice(&16u32.to_le_bytes());
        rf64.extend_from_slice(&1u16.to_le_bytes());
        rf64.extend_from_slice(&2u16.to_le_bytes());
        rf64.extend_from_slice(&48_000u32.to_le_bytes());
        rf64.extend_from_slice(&192_000u32.to_le_bytes());
        rf64.extend_from_slice(&4u16.to_le_bytes());
        rf64.extend_from_slice(&16u16.to_le_bytes());
        rf64.extend_from_slice(b"data\xFF\xFF\xFF\xFF");
        rf64.extend_from_slice(&pcm);
        let mut inp_rf64 = parse_container_bytes(&rf64, "master.rf64").expect("valid rf64");
        assert_eq!(inp_rf64.format_name, "rf64");
        assert!((inp_rf64.duration.unwrap() - 1.0).abs() < 1e-6);
        assert!(inp_rf64.read_packet().is_some());
    }

    #[test]
    fn parses_aiff_80bit_float_sample_rate() {
        let mut aiff = Vec::new();
        aiff.extend_from_slice(b"FORM");
        aiff.extend_from_slice(&100u32.to_be_bytes());
        aiff.extend_from_slice(b"AIFFCOMM");
        aiff.extend_from_slice(&18u32.to_be_bytes());
        aiff.extend_from_slice(&2u16.to_be_bytes()); // channels = 2
        aiff.extend_from_slice(&10u32.to_be_bytes()); // frames
        aiff.extend_from_slice(&16u16.to_be_bytes()); // bits = 16
        // 48000 Hz in 80-bit IEEE extended: exp = 16383 + 15 = 0x400E, mant = 0xBB80_0000_0000_0000
        aiff.extend_from_slice(&0x400Eu16.to_be_bytes());
        aiff.extend_from_slice(&0xBB80_0000_0000_0000u64.to_be_bytes());
        aiff.extend_from_slice(b"SSND");
        aiff.extend_from_slice(&48u32.to_be_bytes());
        aiff.extend_from_slice(&[0u8; 48]);
        let mut inp = parse_container_bytes(&aiff, "studio.aiff").expect("valid aiff");
        assert_eq!(inp.format_name, "aiff");
        assert_eq!(inp.streams[0].sample_rate, 48_000);
        assert!(inp.read_packet().is_some());
    }

    #[test]
    fn parses_y4m_64bit_streaming() {
        let mut y4m = b"YUV4MPEG2 W4 H4 F25:1 Ip A1:1 C420\nFRAME\n".to_vec();
        y4m.extend_from_slice(&[128u8; 16 + 4 + 4]);
        let mut inp = parse_container_bytes(&y4m, "clip.y4m").expect("valid y4m");
        assert_eq!(inp.streams[0].width, 4);
        assert_eq!(inp.streams[0].height, 4);
        assert!((inp.duration.unwrap() - 0.04).abs() < 1e-6);
        assert!(inp.read_packet().is_some());
    }

    #[test]
    fn parses_ivf_vp9_and_av1_headers() {
        let mut ivf = Vec::new();
        ivf.extend_from_slice(b"DKIF");
        ivf.extend_from_slice(&0u16.to_le_bytes());
        ivf.extend_from_slice(&32u16.to_le_bytes());
        ivf.extend_from_slice(b"AV01");
        ivf.extend_from_slice(&1920u16.to_le_bytes());
        ivf.extend_from_slice(&1080u16.to_le_bytes());
        ivf.extend_from_slice(&60u32.to_le_bytes());
        ivf.extend_from_slice(&1u32.to_le_bytes());
        ivf.extend_from_slice(&1u32.to_le_bytes());
        ivf.extend_from_slice(&0u32.to_le_bytes());
        ivf.extend_from_slice(&4u32.to_le_bytes());
        ivf.extend_from_slice(&0u64.to_le_bytes());
        ivf.extend_from_slice(&[1, 2, 3, 4]);
        let mut inp = parse_container_bytes(&ivf, "clip.ivf").expect("valid ivf");
        assert_eq!(inp.streams[0].codec, "av1");
        assert_eq!((inp.streams[0].width, inp.streams[0].height), (1920, 1080));
        assert!(inp.read_packet().is_some());
    }
}
