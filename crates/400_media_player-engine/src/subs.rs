//! Decoding of subtitle tracks embedded in the container: both text (SubRip, ASS/SSA, WebVTT,
//! mov_text…) and bitmap (PGS Blu-ray, VobSub DVD, DVB).
//!
//! The demuxer feeds every packet of the selected track to [`EmbeddedSubs`]; decoded cues are
//! merged (de-duplicated) into a [`SubtitleTrack`] the UI reads from. Because cues accumulate as
//! the container is read, seeking back and forth never loses what was already seen.

use std::sync::Arc;

use media_player_400_core::subtitle::{AssHeader, BitmapRect, Cue, SubtitleTrack, ass_event_to_cue};
use parking_lot::Mutex;

use crate::codecs::{SubRect, SubtitleDecoder};
use crate::container::{ContainerInput, MediaPacket};
use crate::error::{EngineError, Result};

/// Text cues shown when the container gives no duration (rare; ends at the next cue anyway).
const FALLBACK_DURATION: f64 = 4.0;
/// Upper bound for a bitmap cue whose end is signalled by a later clear packet (PGS / DVB).
const FALLBACK_BITMAP_DURATION: f64 = 12.0;

pub(crate) struct EmbeddedSubs {
    pub index: usize,
    decoder: SubtitleDecoder,
    time_base: f64,
    header: AssHeader,
    track: Arc<Mutex<SubtitleTrack>>,
}

impl EmbeddedSubs {
    /// Opens a decoder for stream `index` of `input`; decoded cues land in `track`.
    pub fn open(input: &ContainerInput, index: usize, video_size: Option<(u32, u32)>, track: Arc<Mutex<SubtitleTrack>>) -> Result<Self> {
        let stream = input.stream(index).ok_or_else(|| EngineError::Other(format!("no stream #{index}")))?;
        let fallback_canvas = video_size.unwrap_or((1920, 1080));
        let decoder = SubtitleDecoder::new(stream, fallback_canvas);
        let header = if stream.extra_data.is_empty() {
            AssHeader::default()
        } else {
            AssHeader::parse(&String::from_utf8_lossy(&stream.extra_data))
        };
        Ok(Self { index, decoder, time_base: stream.time_base_sec(), header, track })
    }

    /// Forgets decoder state after a seek (cues already collected are kept).
    pub fn flush(&mut self) {
        self.decoder.flush();
    }

    /// Decodes one packet of the track. `start_offset` is the container's start time.
    pub fn feed(&mut self, pkt: &MediaPacket, start_offset: f64) {
        let Some(sub) = self.decoder.decode(pkt) else { return };
        let base = pkt.pts().or(pkt.dts()).map(|p| p as f64 * self.time_base).unwrap_or(0.0) - start_offset;
        let start = base + f64::from(sub.start_ms) / 1000.0;
        let pkt_dur = (pkt.duration() > 0).then(|| pkt.duration() as f64 * self.time_base);
        let end = match sub.end_ms {
            0 | u32::MAX => start + pkt_dur.unwrap_or(FALLBACK_DURATION),
            e => base + f64::from(e) / 1000.0,
        };

        if sub.is_clear || sub.rects.is_empty() {
            self.track.lock().close_bitmaps_at(start);
            return;
        }

        let mut cues: Vec<Cue> = Vec::new();
        let mut bitmaps: Vec<BitmapRect> = Vec::new();
        for rect in sub.rects {
            match rect {
                SubRect::Ass(a) => cues.extend(ass_event_to_cue(&a, start, end, &self.header)),
                SubRect::Text(t) => {
                    let c = Cue::from_text(start, end, &t);
                    if !c.lines.is_empty() {
                        cues.push(c);
                    }
                }
                SubRect::Bitmap(bm) => bitmaps.push(bm),
            }
        }
        if !cues.is_empty() || !bitmaps.is_empty() {
            let mut track = self.track.lock();
            for c in cues {
                track.push(c);
            }
            if !bitmaps.is_empty() {
                let bmp_end = match sub.end_ms {
                    0 | u32::MAX => start + pkt_dur.unwrap_or(FALLBACK_BITMAP_DURATION),
                    e => base + f64::from(e) / 1000.0,
                };
                track.close_bitmaps_at(start);
                track.push(Cue::from_bitmaps(start, bmp_end, bitmaps));
            }
        }
    }
}
