//! Zek Music — player de músicas leve para Linux.
//! Pasta = playlist, monitoramento automático, playlists personalizadas.

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Layout, Margin, Pos2, Rect, RichText, Rounding,
    Sense, Stroke, Vec2,
};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use rand::Rng;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder as SymDecoder, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use std::time::{Duration, Instant};

// ───────────────────────── Cores (azul + escuro) ─────────────────────────
const BG: Color32 = Color32::from_rgb(8, 12, 24);
const PANEL: Color32 = Color32::from_rgb(11, 17, 35);
const BTN: Color32 = Color32::from_rgb(24, 36, 70);
const BTN_HOVER: Color32 = Color32::from_rgb(34, 50, 96);
const RAIL: Color32 = Color32::from_rgb(36, 50, 92);
const ROW_HOVER: Color32 = Color32::from_rgb(18, 28, 56);
const SEL_BG: Color32 = Color32::from_rgb(22, 40, 86);
const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
const ACCENT_HI: Color32 = Color32::from_rgb(96, 165, 250);
const CYAN: Color32 = Color32::from_rgb(34, 211, 238);
const TEXT: Color32 = Color32::from_rgb(226, 233, 246);
const MUTED: Color32 = Color32::from_rgb(124, 140, 175);
const WARN: Color32 = Color32::from_rgb(251, 191, 100);

const AUDIO_EXT: &[&str] = &["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac"];

const PALETTES: [([u8; 3], [u8; 3]); 8] = [
    ([37, 99, 235], [56, 189, 248]),
    ([79, 70, 229], [96, 165, 250]),
    ([14, 116, 144], [45, 212, 191]),
    ([30, 64, 175], [129, 140, 248]),
    ([2, 132, 199], [34, 211, 238]),
    ([67, 56, 202], [192, 132, 252]),
    ([29, 78, 216], [103, 232, 249]),
    ([12, 74, 110], [56, 189, 248]),
];

// ───────────────────────── Dados / configuração ─────────────────────────
#[derive(Serialize, Deserialize, Clone)]
enum PlaylistSource {
    /// Playlist automática: vem de uma pasta no disco.
    Folder(PathBuf),
    /// Playlist personalizada: lista de arquivos escolhidos pelo usuário.
    Custom(Vec<PathBuf>),
}

#[derive(Serialize, Deserialize, Clone)]
struct Playlist {
    id: u64,
    name: String,
    source: PlaylistSource,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Config {
    roots: Vec<PathBuf>,
    playlists: Vec<Playlist>, // a ordem do vetor é a ordem exibida
    next_id: u64,
    volume: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self { roots: vec![], playlists: vec![], next_id: 0, volume: 0.8 }
    }
}

#[derive(Clone, Default)]
struct Meta {
    title: String,
    artist: String,
    album: String,
    duration: f32,
}

fn config_path() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("zek-music");
    p.push("config.json");
    p
}

fn load_config() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ───────────────────────── Utilidades ─────────────────────────
fn is_audio(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map_or(false, |e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

fn folder_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// Remove sufixos como "(MP3_160K)" dos nomes baixados de conversores.
fn clean_title(s: &str) -> String {
    let t = s.trim();
    if t.ends_with(')') {
        if let Some(i) = t.rfind("(MP3_") {
            let cut = t[..i].trim();
            if !cut.is_empty() {
                return cut.to_string();
            }
        }
    }
    t.to_string()
}

fn fmt_time(t: f32) -> String {
    let s = t.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

fn fmt_total(t: f32) -> String {
    let m = (t / 60.0).round() as u32;
    if m >= 60 {
        format!("{} h {} min", m / 60, m % 60)
    } else {
        format!("{} min", m)
    }
}

fn initial(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
}

fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222317;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn tile_colors(seed: &str) -> (Color32, Color32) {
    let p = PALETTES[(hash_str(seed) % 8) as usize];
    (Color32::from_rgb(p.0[0], p.0[1], p.0[2]), Color32::from_rgb(p.1[0], p.1[1], p.1[2]))
}

fn stroke(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

// ───────────────────────── Escaneamento de pastas ─────────────────────────
fn collect_audio(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if is_hidden(&e.file_name()) {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                collect_audio(&p, out, depth + 1);
            }
        } else if is_audio(&p) {
            out.push(p);
        }
    }
}

fn sort_tracks(v: &mut Vec<PathBuf>) {
    v.sort_by_key(|p| p.to_string_lossy().to_lowercase());
}

/// Cada subpasta (com músicas) de uma pasta raiz vira uma playlist.
/// Músicas soltas direto na raiz formam uma playlist com o nome da raiz.
fn scan_roots(roots: &[PathBuf]) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut map = HashMap::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut loose = vec![];
        for e in rd.flatten() {
            if is_hidden(&e.file_name()) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                let mut v = vec![];
                collect_audio(&p, &mut v, 0);
                if !v.is_empty() {
                    sort_tracks(&mut v);
                    map.insert(p, v);
                }
            } else if is_audio(&p) {
                loose.push(p);
            }
        }
        if !loose.is_empty() {
            sort_tracks(&mut loose);
            map.insert(root.clone(), loose);
        }
    }
    map
}

fn read_info(path: &Path, fallback: Option<f32>, want_cover: bool) -> (Meta, Option<Vec<u8>>) {
    let mut m = Meta { title: clean_title(&stem(path)), duration: fallback.unwrap_or(0.0), ..Default::default() };
    let mut cover = None;
    if let Ok(tf) = lofty::read_from_path(path) {
        let d = tf.properties().duration().as_secs_f32();
        if d > 0.0 {
            m.duration = d;
        }
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            if let Some(t) = tag.title() {
                let t = clean_title(&t);
                if !t.is_empty() {
                    m.title = t;
                }
            }
            if let Some(a) = tag.artist() {
                m.artist = a.to_string();
            }
            if let Some(a) = tag.album() {
                m.album = a.to_string();
            }
            if want_cover {
                if let Some(pic) = tag.pictures().first() {
                    cover = Some(pic.data().to_vec());
                }
            }
        }
    }
    (m, cover)
}

fn make_texture(ctx: &egui::Context, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.thumbnail(320, 320).to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("cover", ci, egui::TextureOptions::LINEAR))
}

// ───────────────────────── Decodificação (com plano B) ─────────────────────────
/// Pula as etiquetas ID3v2 e vai direto para o primeiro quadro MPEG válido.
fn strip_tags(data: &[u8]) -> Vec<u8> {
    let mut start = 0usize;
    while data.len() >= start + 10 && &data[start..start + 3] == b"ID3" {
        let size = (((data[start + 6] & 0x7f) as usize) << 21)
            | (((data[start + 7] & 0x7f) as usize) << 14)
            | (((data[start + 8] & 0x7f) as usize) << 7)
            | ((data[start + 9] & 0x7f) as usize);
        let footer = if data[start + 5] & 0x10 != 0 { 10 } else { 0 };
        start += 10 + size + footer;
    }
    let mut i = start;
    while i + 3 < data.len() {
        let b1 = data[i + 1];
        let b2 = data[i + 2];
        if data[i] == 0xFF
            && (b1 & 0xE0) == 0xE0
            && ((b1 >> 3) & 3) != 1
            && ((b1 >> 1) & 3) != 0
            && (b2 >> 4) != 0xF
            && (b2 >> 4) != 0
            && ((b2 >> 2) & 3) != 3
        {
            return data[i..].to_vec();
        }
        i += 1;
    }
    data[start.min(data.len())..].to_vec()
}

const MP3_BR_V1: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
const MP3_BR_V2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// Se existir um quadro MPEG Layer III válido em `i`, devolve (tamanho_do_quadro, main_data_begin).
fn mp3_frame_at(d: &[u8], i: usize) -> Option<(usize, usize)> {
    if i + 4 > d.len() {
        return None;
    }
    let (b1, b2) = (d[i + 1], d[i + 2]);
    if d[i] != 0xFF || (b1 & 0xE0) != 0xE0 {
        return None;
    }
    let version = (b1 >> 3) & 3; // 3 = MPEG1, 2 = MPEG2, 0 = MPEG2.5
    let layer = (b1 >> 1) & 3; // 1 = Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let br_idx = (b2 >> 4) as usize;
    let sr_idx = ((b2 >> 2) & 3) as usize;
    if br_idx == 0 || br_idx == 15 || sr_idx == 3 {
        return None;
    }
    let mpeg1 = version == 3;
    let br = (if mpeg1 { MP3_BR_V1[br_idx] } else { MP3_BR_V2[br_idx] }) as usize * 1000;
    let sr_base = [44100usize, 48000, 32000][sr_idx];
    let sr = match version {
        3 => sr_base,
        2 => sr_base / 2,
        _ => sr_base / 4,
    };
    let pad = ((b2 >> 1) & 1) as usize;
    let len = if mpeg1 { 144 * br / sr + pad } else { 72 * br / sr + pad };
    let crc = if b1 & 1 == 0 { 2 } else { 0 };
    let off = i + 4 + crc;
    let mdb = if mpeg1 {
        if off + 1 >= d.len() {
            return None;
        }
        ((d[off] as usize) << 1) | ((d[off + 1] as usize) >> 7)
    } else {
        if off >= d.len() {
            return None;
        }
        d[off] as usize
    };
    Some((len, mdb))
}

/// Posições de quadros que não dependem de dados anteriores (bons pontos de partida).
fn mp3_clean_starts(d: &[u8], max: usize) -> Vec<usize> {
    let mut out = vec![];
    let mut i = 0usize;
    let mut frames = 0;
    while i + 4 < d.len() && out.len() < max && frames < 3000 {
        match mp3_frame_at(d, i) {
            Some((len, mdb)) if len > 4 => {
                let next_ok = i + len + 4 >= d.len() || mp3_frame_at(d, i + len).is_some();
                if next_ok {
                    if mdb == 0 {
                        out.push(i);
                    }
                    i += len;
                    frames += 1;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Primeiro quadro MPEG válido (confirmado por mais dois quadros seguidos).
fn mp3_first_frame(d: &[u8]) -> Option<usize> {
    let limit = d.len().min(1 << 20);
    for i in 0..limit {
        if let Some((l1, _)) = mp3_frame_at(d, i) {
            if l1 > 4 {
                if let Some((l2, _)) = mp3_frame_at(d, i + l1) {
                    if l2 > 4 && mp3_frame_at(d, i + l1 + l2).is_some() {
                        return Some(i);
                    }
                }
            }
        }
    }
    None
}

/// Coloca 3 quadros de silêncio antes do áudio. Alguns MP3 começam num quadro que
/// depende de dados anteriores (que não existem); os quadros de silêncio preenchem
/// esse "reservatório" e o decodificador consegue começar.
fn mp3_with_silent_prefix(d: &[u8]) -> Option<Vec<u8>> {
    let i = mp3_first_frame(d)?;
    let (len, _) = mp3_frame_at(d, i)?;
    let pad = ((d[i + 2] >> 1) & 1) as usize;
    let base = len.checked_sub(pad)?;
    if base < 24 {
        return None;
    }
    // mesmo cabeçalho, sem CRC e sem padding
    let header = [d[i], d[i + 1] | 1, d[i + 2] & !0x02, d[i + 3]];
    let mut out = Vec::with_capacity(d.len() - i + base * 3);
    for _ in 0..3 {
        out.extend_from_slice(&header);
        out.resize(out.len() + base - 4, 0);
    }
    out.extend_from_slice(&d[i..]);
    Some(out)
}

/// Decodificador "tolerante": usa o symphonia direto e PULA os blocos com defeito
/// em vez de encerrar a música (o rodio para no primeiro erro).
struct TolerantSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymDecoder>,
    track_id: u32,
    buf: Vec<i16>,
    pos: usize,
    channels: u16,
    rate: u32,
    dur: Option<Duration>,
}

impl TolerantSource {
    fn new(data: Vec<u8>) -> Option<Self> {
        let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(data)), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .ok()?;
        let format = probed.format;
        let (track_id, params) = {
            let t = format.default_track()?;
            (t.id, t.codec_params.clone())
        };
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).ok()?;
        let rate = params.sample_rate.unwrap_or(44100);
        let channels = params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let dur = match (params.n_frames, params.sample_rate) {
            (Some(n), Some(r)) if r > 0 => Some(Duration::from_secs_f64(n as f64 / r as f64)),
            _ => None,
        };
        let mut s = Self { format, decoder, track_id, buf: vec![], pos: 0, channels, rate, dur };
        if !s.refill() {
            return None;
        }
        Some(s)
    }

    fn refill(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(_) => return false,
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let mut sb = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
                    sb.copy_interleaved_ref(decoded);
                    self.channels = spec.channels.count() as u16;
                    self.rate = spec.rate;
                    self.buf.clear();
                    self.buf.extend_from_slice(sb.samples());
                    self.pos = 0;
                    if self.buf.is_empty() {
                        continue;
                    }
                    return true;
                }
                // bloco com defeito: ignora e segue para o próximo
                Err(SymError::DecodeError(_)) => continue,
                Err(_) => return false,
            }
        }
    }
}

impl Iterator for TolerantSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        if self.pos >= self.buf.len() && !self.refill() {
            return None;
        }
        let v = self.buf[self.pos];
        self.pos += 1;
        Some(v)
    }
}

impl Source for TolerantSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.dur
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        match self.format.seek(
            SeekMode::Coarse,
            SeekTo::Time { time: Time::from(pos.as_secs_f64()), track_id: Some(self.track_id) },
        ) {
            Ok(_) => {
                self.decoder.reset();
                self.buf.clear();
                self.pos = 0;
                Ok(())
            }
            Err(_) => Err(rodio::source::SeekError::NotSupported { underlying_source: "TolerantSource" }),
        }
    }
}

fn make_source(path: &Path) -> Result<Box<dyn Source<Item = i16> + Send>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    match Decoder::new(std::io::BufReader::new(file)) {
        Ok(d) => Ok(Box::new(d)),
        Err(first) => {
            // Plano B: decodificador tolerante (pula blocos com defeito e continua tocando).
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            let cleaned = strip_tags(&data);
            if let Some(t) = TolerantSource::new(cleaned.clone()) {
                return Ok(Box::new(t));
            }
            if let Some(fixed) = mp3_with_silent_prefix(&cleaned) {
                if let Some(t) = TolerantSource::new(fixed) {
                    return Ok(Box::new(t));
                }
            }
            if let Some(t) = TolerantSource::new(data) {
                return Ok(Box::new(t));
            }
            Err(format!("{first}"))
        }
    }
}

// ───────────────────────── Desenho (gradientes, ícones, barras) ─────────────────────────
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn rounded_gradient(p: &egui::Painter, rect: Rect, r: f32, c1: Color32, c2: Color32) {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (rect.right() - r, rect.top() + r, -90.0_f32),
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, 90.0),
        (rect.left() + r, rect.top() + r, 180.0),
    ];
    let mut pts: Vec<Pos2> = vec![];
    for (cx, cy, start) in corners {
        for i in 0..=6 {
            let a = (start + i as f32 * 15.0).to_radians();
            pts.push(Pos2::new(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    let col = |p: Pos2| -> Color32 {
        let t = ((p.x - rect.left()) / rect.width().max(1.0) + (p.y - rect.top()) / rect.height().max(1.0)) / 2.0;
        lerp_color(c1, c2, t.clamp(0.0, 1.0))
    };
    let mut mesh = egui::Mesh::default();
    let center = rect.center();
    mesh.colored_vertex(center, col(center));
    for q in &pts {
        mesh.colored_vertex(*q, col(*q));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + ((i + 1) % n));
    }
    p.add(egui::Shape::mesh(mesh));
}

fn gradient_rect_v(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), top);
    m.colored_vertex(rect.right_top(), top);
    m.colored_vertex(rect.right_bottom(), bottom);
    m.colored_vertex(rect.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

fn equalizer(p: &egui::Painter, center: Pos2, t: f64, animate: bool) {
    for i in 0..3 {
        let h = if animate { 4.0 + 9.0 * (((t * 6.0 + i as f64 * 1.7).sin() * 0.5 + 0.5) as f32) } else { 5.0 };
        let x = center.x - 6.0 + i as f32 * 6.0;
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x - 1.6, center.y + 7.0 - h), Pos2::new(x + 1.6, center.y + 7.0)),
            1.0,
            ACCENT_HI,
        );
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Icon {
    Play,
    Pause,
    Next,
    Prev,
    Shuffle,
    Repeat,
    RepeatOne,
    Volume,
    Muted,
}

fn arrow_head(p: &egui::Painter, tip: Pos2, s: f32, col: Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![Pos2::new(tip.x + 0.7 * s, tip.y), Pos2::new(tip.x - 0.3 * s, tip.y - 0.7 * s), Pos2::new(tip.x - 0.3 * s, tip.y + 0.7 * s)],
        col,
        Stroke::NONE,
    ));
}

fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, accent: bool, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hov = resp.hovered();
    let toggle_like = matches!(icon, Icon::Shuffle | Icon::Repeat | Icon::RepeatOne | Icon::Volume | Icon::Muted);
    let col = if accent {
        Color32::WHITE
    } else if active {
        ACCENT_HI
    } else if hov {
        Color32::WHITE
    } else if toggle_like {
        MUTED
    } else {
        TEXT
    };
    let p = ui.painter();
    if accent {
        p.circle_filled(rect.center(), size / 2.0, if hov { ACCENT_HI } else { ACCENT });
    }
    let c = rect.center();
    let s = size * 0.2;
    let poly = |pts: Vec<Pos2>| egui::Shape::convex_polygon(pts, col, Stroke::NONE);
    match icon {
        Icon::Play => {
            p.add(poly(vec![Pos2::new(c.x - 0.8 * s, c.y - 1.1 * s), Pos2::new(c.x - 0.8 * s, c.y + 1.1 * s), Pos2::new(c.x + 1.1 * s, c.y)]));
        }
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - 0.9 * s, c.y - 1.1 * s), Pos2::new(c.x - 0.25 * s, c.y + 1.1 * s)), 1.0, col);
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x + 0.25 * s, c.y - 1.1 * s), Pos2::new(c.x + 0.9 * s, c.y + 1.1 * s)), 1.0, col);
        }
        Icon::Next => {
            p.add(poly(vec![Pos2::new(c.x - 1.1 * s, c.y - s), Pos2::new(c.x - 1.1 * s, c.y + s), Pos2::new(c.x + 0.5 * s, c.y)]));
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x + 0.8 * s, c.y - s), Pos2::new(c.x + 1.3 * s, c.y + s)), 1.0, col);
        }
        Icon::Prev => {
            p.add(poly(vec![Pos2::new(c.x + 1.1 * s, c.y - s), Pos2::new(c.x + 1.1 * s, c.y + s), Pos2::new(c.x - 0.5 * s, c.y)]));
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - 1.3 * s, c.y - s), Pos2::new(c.x - 0.8 * s, c.y + s)), 1.0, col);
        }
        Icon::Shuffle => {
            let u = size * 0.17;
            let st = stroke(1.8, col);
            p.line_segment([Pos2::new(c.x - 1.6 * u, c.y - 1.0 * u), Pos2::new(c.x + 1.2 * u, c.y + 1.0 * u)], st);
            p.line_segment([Pos2::new(c.x - 1.6 * u, c.y + 1.0 * u), Pos2::new(c.x + 1.2 * u, c.y - 1.0 * u)], st);
            arrow_head(p, Pos2::new(c.x + 1.4 * u, c.y + 1.0 * u), u * 1.2, col);
            arrow_head(p, Pos2::new(c.x + 1.4 * u, c.y - 1.0 * u), u * 1.2, col);
        }
        Icon::Repeat | Icon::RepeatOne => {
            let u = size * 0.17;
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(3.6 * u, 2.0 * u)), 0.9 * u, stroke(1.8, col));
            arrow_head(p, Pos2::new(c.x + 0.5 * u, c.y - 1.0 * u), u * 1.3, col);
            if icon == Icon::RepeatOne {
                p.text(c, Align2::CENTER_CENTER, "1", FontId::proportional(size * 0.34), col);
            }
        }
        Icon::Volume | Icon::Muted => {
            let u = size * 0.15;
            p.rect_filled(Rect::from_min_max(Pos2::new(c.x - 2.0 * u, c.y - 0.8 * u), Pos2::new(c.x - 0.9 * u, c.y + 0.8 * u)), 1.0, col);
            p.add(poly(vec![
                Pos2::new(c.x - 0.9 * u, c.y - 0.8 * u),
                Pos2::new(c.x + 0.4 * u, c.y - 1.7 * u),
                Pos2::new(c.x + 0.4 * u, c.y + 1.7 * u),
                Pos2::new(c.x - 0.9 * u, c.y + 0.8 * u),
            ]));
            if icon == Icon::Volume {
                for r in [1.3_f32, 2.2] {
                    let pts: Vec<Pos2> = (0..=8)
                        .map(|i| {
                            let a = (-45.0_f32 + i as f32 * 11.25).to_radians();
                            Pos2::new(c.x + 0.4 * u + r * u * a.cos(), c.y + r * u * a.sin())
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, stroke(1.6, col)));
                }
            } else {
                let st = stroke(1.8, col);
                p.line_segment([Pos2::new(c.x + 1.2 * u, c.y - 1.0 * u), Pos2::new(c.x + 2.8 * u, c.y + 1.0 * u)], st);
                p.line_segment([Pos2::new(c.x + 1.2 * u, c.y + 1.0 * u), Pos2::new(c.x + 2.8 * u, c.y - 1.0 * u)], st);
            }
        }
    }
    if active && toggle_like {
        p.circle_filled(Pos2::new(c.x, rect.bottom() - 1.5), 1.8, ACCENT_HI);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn arrow_btn(ui: &mut egui::Ui, rect: Rect, up: bool, id: egui::Id) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    let c = rect.center();
    let s = 4.5;
    let col = if resp.hovered() { Color32::WHITE } else { MUTED };
    let pts = if up {
        vec![Pos2::new(c.x - s, c.y + s * 0.6), Pos2::new(c.x + s, c.y + s * 0.6), Pos2::new(c.x, c.y - s * 0.8)]
    } else {
        vec![Pos2::new(c.x - s, c.y - s * 0.6), Pos2::new(c.x + s, c.y - s * 0.6), Pos2::new(c.x, c.y + s * 0.8)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

struct BarOut {
    changed: bool,
    released: bool,
    dragging: bool,
}

/// Barra de progresso/volume desenhada à mão (clique e arraste).
fn bar(ui: &mut egui::Ui, value: &mut f32, max: f32, width: f32, enabled: bool) -> BarOut {
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 16.0), sense);
    let mut out = BarOut { changed: false, released: false, dragging: false };
    if enabled {
        if resp.dragged() || resp.drag_started() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let t = ((pos.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                *value = t * max;
                out.changed = true;
            }
        }
        out.dragging = resp.dragged();
        out.released = resp.drag_stopped() || resp.clicked();
    }
    let hot = enabled && (resp.hovered() || resp.dragged());
    let frac = if max > 0.0 { (*value / max).clamp(0.0, 1.0) } else { 0.0 };
    let cy = rect.center().y;
    let rail_h = if hot { 6.0 } else { 4.0 };
    let p = ui.painter();
    let rail = Rect::from_min_max(Pos2::new(rect.left(), cy - rail_h / 2.0), Pos2::new(rect.right(), cy + rail_h / 2.0));
    p.rect_filled(rail, rail_h / 2.0, RAIL);
    let fill = Rect::from_min_max(rail.min, Pos2::new(rail.left() + rail.width() * frac, rail.max.y));
    if enabled {
        p.rect_filled(fill, rail_h / 2.0, if hot { ACCENT_HI } else { ACCENT });
    }
    if hot {
        p.circle_filled(Pos2::new(fill.right(), cy), 6.5, Color32::WHITE);
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    out
}

fn columns(rect: Rect) -> (f32, f32, f32) {
    let title_x = rect.left() + 60.0;
    let usable = (rect.width() - 60.0 - 120.0).max(100.0);
    let artist_x = title_x + usable * 0.58;
    let dur_x = rect.right() - 28.0;
    (title_x, artist_x, dur_x)
}

// ───────────────────────── Estado do app ─────────────────────────
#[derive(Clone, Copy, PartialEq)]
enum Repeat {
    Off,
    All,
    One,
}

// ───────────────────────── Motor de áudio (thread própria) ─────────────────────────
// Roda separado da janela: continua tocando e trocando de música mesmo com o
// programa em segundo plano (ex.: dentro de um jogo) e recebe as teclas de mídia.
enum Cmd {
    PlayQueue { queue: Vec<PathBuf>, index: usize, playlist: Option<u64> },
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    Stop,
    Seek(f32),
    Volume(f32),
    Shuffle(bool),
    Repeat(Repeat),
}

#[derive(Default)]
struct Shared {
    now: Option<Meta>,
    cover: Option<Vec<u8>>,
    cover_gen: u64,
    active: bool,
    paused: bool,
    pos: f32,
    cur_path: Option<PathBuf>,
    queue_pl: Option<u64>,
    has_queue: bool,
    status: String,
}

#[derive(Default, Clone)]
struct Snap {
    now: Option<Meta>,
    active: bool,
    paused: bool,
    pos: f32,
    cur_path: Option<PathBuf>,
    queue_pl: Option<u64>,
    has_queue: bool,
    status: String,
}

fn make_controls(tx: Sender<Cmd>) -> Option<MediaControls> {
    let cfg = PlatformConfig { dbus_name: "zek_music", display_name: "Zek Music", hwnd: None };
    let mut c = MediaControls::new(cfg).ok()?;
    let r = c.attach(move |ev: MediaControlEvent| {
        let cmd = match ev {
            MediaControlEvent::Play => Cmd::Play,
            MediaControlEvent::Pause => Cmd::Pause,
            MediaControlEvent::Toggle => Cmd::Toggle,
            MediaControlEvent::Next => Cmd::Next,
            MediaControlEvent::Previous => Cmd::Prev,
            MediaControlEvent::Stop => Cmd::Stop,
            MediaControlEvent::SetPosition(MediaPosition(d)) => Cmd::Seek(d.as_secs_f32()),
            _ => return,
        };
        let _ = tx.send(cmd);
    });
    if r.is_err() {
        return None;
    }
    Some(c)
}

struct Engine {
    rx: Receiver<Cmd>,
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    stream: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    queue: Vec<PathBuf>,
    qindex: usize,
    queue_pl: Option<u64>,
    paused: bool,
    shuffle: bool,
    repeat: Repeat,
    volume: f32,
    last_pos: f32,
    now: Option<Meta>,
    controls: Option<MediaControls>,
}

impl Engine {
    fn new(rx: Receiver<Cmd>, tx: Sender<Cmd>, shared: Arc<Mutex<Shared>>, ctx: egui::Context, volume: f32) -> Self {
        Self {
            rx,
            shared,
            ctx,
            stream: OutputStream::try_default().ok(),
            sink: None,
            queue: vec![],
            qindex: 0,
            queue_pl: None,
            paused: false,
            shuffle: false,
            repeat: Repeat::Off,
            volume,
            last_pos: 0.0,
            now: None,
            controls: make_controls(tx),
        }
    }

    fn run(mut self) {
        loop {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => self.handle(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
    }

    fn tick(&mut self) {
        let ended = !self.paused && self.sink.as_ref().map_or(false, |s| s.empty());
        if ended {
            let early = self.last_pos < 1.5 && self.now.as_ref().map_or(false, |n| n.duration > 5.0);
            let nome = self.now.as_ref().map(|n| n.title.clone()).unwrap_or_default();
            self.advance(true);
            if early {
                self.set_status(format!("\"{}\" parou logo no começo (arquivo com defeito?). Pulei para a próxima.", nome));
                self.publish_full();
            }
        } else {
            self.publish_pos();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::PlayQueue { queue, index, playlist } => {
                self.queue = queue;
                self.queue_pl = playlist;
                self.play_index(index);
            }
            Cmd::Play => {
                if self.sink.is_none() || self.paused {
                    self.toggle();
                }
            }
            Cmd::Pause => {
                if self.sink.is_some() && !self.paused {
                    self.toggle();
                }
            }
            Cmd::Toggle => self.toggle(),
            Cmd::Next => self.advance(false),
            Cmd::Prev => self.prev(),
            Cmd::Stop => self.stop(),
            Cmd::Seek(t) => {
                if let Some(s) = &self.sink {
                    let _ = s.try_seek(Duration::from_secs_f32(t.max(0.0)));
                }
                self.publish_full();
                self.mpris_update(false);
            }
            Cmd::Volume(v) => {
                self.volume = v;
                if let Some(s) = &self.sink {
                    s.set_volume(v);
                }
            }
            Cmd::Shuffle(b) => self.shuffle = b,
            Cmd::Repeat(r) => self.repeat = r,
        }
    }

    fn publish_full(&self) {
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        {
            let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            g.now = self.now.clone();
            g.active = self.sink.is_some();
            g.paused = self.paused;
            g.pos = pos;
            g.cur_path = if self.sink.is_some() { self.queue.get(self.qindex).cloned() } else { None };
            g.queue_pl = self.queue_pl;
            g.has_queue = !self.queue.is_empty();
        }
        self.ctx.request_repaint();
    }

    fn publish_pos(&mut self) {
        if let Some(s) = &self.sink {
            let pos = s.get_pos().as_secs_f32();
            self.last_pos = pos;
            let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            g.pos = pos;
        }
    }

    fn set_status(&self, s: String) {
        let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        g.status = s;
    }

    fn set_cover(&self, c: Option<Vec<u8>>) {
        let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        g.cover = c;
        g.cover_gen += 1;
    }

    fn mpris_update(&mut self, meta_changed: bool) {
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        let stopped = self.sink.is_none();
        let paused = self.paused;
        if let Some(c) = self.controls.as_mut() {
            if meta_changed {
                if let Some(n) = &self.now {
                    let _ = c.set_metadata(MediaMetadata {
                        title: Some(n.title.as_str()),
                        artist: Some(n.artist.as_str()),
                        album: Some(n.album.as_str()),
                        cover_url: None,
                        duration: Some(Duration::from_secs_f32(n.duration.max(0.0))),
                    });
                }
            }
            let progress = Some(MediaPosition(Duration::from_secs_f32(pos.max(0.0))));
            let pb = if stopped {
                MediaPlayback::Stopped
            } else if paused {
                MediaPlayback::Paused { progress }
            } else {
                MediaPlayback::Playing { progress }
            };
            let _ = c.set_playback(pb);
        }
    }

    fn play_index(&mut self, idx: usize) -> bool {
        if idx >= self.queue.len() {
            return false;
        }
        let path = self.queue[idx].clone();
        self.qindex = idx;
        let Some((_, handle)) = &self.stream else {
            self.set_status("Nenhuma saída de áudio encontrada.".into());
            self.publish_full();
            return false;
        };
        let source = match make_source(&path) {
            Ok(s) => s,
            Err(e) => {
                self.set_status(format!("Não foi possível tocar \"{}\": {}", clean_title(&stem(&path)), e));
                self.publish_full();
                return false;
            }
        };
        let src_dur = source.total_duration().map(|d| d.as_secs_f32());
        let sink = match Sink::try_new(handle) {
            Ok(s) => s,
            Err(_) => {
                self.set_status("Erro ao iniciar o áudio.".into());
                self.publish_full();
                return false;
            }
        };
        sink.set_volume(self.volume);
        sink.append(source);
        if let Some(old) = self.sink.take() {
            old.stop();
        }
        self.sink = Some(sink);
        let (m, cover) = read_info(&path, src_dur, true);
        self.set_cover(cover);
        self.now = Some(m);
        self.last_pos = 0.0;
        self.paused = false;
        self.set_status(String::new());
        self.publish_full();
        self.mpris_update(true);
        true
    }

    fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.now = None;
        self.paused = false;
        self.set_cover(None);
        self.publish_full();
        self.mpris_update(true);
    }

    fn toggle(&mut self) {
        if let Some(s) = &self.sink {
            if self.paused {
                s.play();
            } else {
                s.pause();
            }
            self.paused = !self.paused;
            self.publish_full();
            self.mpris_update(false);
        } else if !self.queue.is_empty() {
            let i = self.qindex.min(self.queue.len() - 1);
            self.play_index(i);
        }
    }

    fn pick_next(&self, cur: usize, auto: bool, allow_one: bool) -> Option<usize> {
        let n = self.queue.len();
        if n == 0 {
            return None;
        }
        if auto && allow_one && self.repeat == Repeat::One {
            return Some(cur);
        }
        if self.shuffle && n > 1 {
            let mut rng = rand::thread_rng();
            let mut r = cur;
            while r == cur {
                r = rng.gen_range(0..n);
            }
            return Some(r);
        }
        if cur + 1 < n {
            Some(cur + 1)
        } else if self.repeat == Repeat::All || !auto {
            Some(0)
        } else {
            None
        }
    }

    fn advance(&mut self, auto: bool) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let mut cur = self.qindex;
        let mut first = true;
        for _ in 0..n.min(10) {
            let Some(next) = self.pick_next(cur, auto || !first, first) else {
                self.stop();
                return;
            };
            if self.play_index(next) {
                return;
            }
            cur = next;
            first = false;
        }
        self.stop();
    }

    fn prev(&mut self) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let pos = self.sink.as_ref().map_or(0.0, |s| s.get_pos().as_secs_f32());
        if pos > 3.0 {
            if let Some(s) = &self.sink {
                let _ = s.try_seek(Duration::ZERO);
            }
            self.publish_full();
        } else {
            let i = if self.qindex == 0 { n - 1 } else { self.qindex - 1 };
            self.play_index(i);
        }
    }
}

enum ScanMsg {
    Folders(HashMap<PathBuf, Vec<PathBuf>>),
    Metas(Vec<(PathBuf, Meta)>),
    Done,
}

enum Act {
    Select(u64),
    StartRename(u64),
    CommitRename,
    CancelRename,
    MoveUp(usize),
    MoveDown(usize),
    Delete(u64),
    ResetName(u64),
    NewPlaylist,
    NewWith(PathBuf),
    AddFolders,
    RemoveRoot(usize),
    Refresh,
    PlayAt(usize),
    PlayAll,
    AddTo(u64, PathBuf),
    RemoveTrack(usize),
    AddFiles,
    Toggle,
    Next,
    Prev,
    Seek(f32),
    VolChanged,
}

struct ZekApp {
    ctx: egui::Context,
    cfg: Config,
    tracks: HashMap<PathBuf, Vec<PathBuf>>,
    meta: HashMap<PathBuf, Meta>,
    selected: Option<u64>,
    renaming: Option<(u64, String)>,
    rename_focus: bool,
    search: String,
    show_folders: bool,

    cmd_tx: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    snap: Snap,
    cover: Option<egui::TextureHandle>,
    cover_gen: u64,
    shuffle: bool,
    repeat: Repeat,
    muted: bool,
    seek_val: f32,
    seeking: bool,

    watcher: Option<RecommendedWatcher>,
    fs_tx: Sender<()>,
    fs_rx: Receiver<()>,
    dirty_since: Option<Instant>,
    scan_rx: Option<Receiver<ScanMsg>>,
}

impl ZekApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let cfg = load_config();
        let (fs_tx, fs_rx) = mpsc::channel();
        let selected = cfg.playlists.first().map(|p| p.id);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let shared = Arc::new(Mutex::new(Shared::default()));
        {
            let (sh, cx, tx, vol) = (shared.clone(), cc.egui_ctx.clone(), cmd_tx.clone(), cfg.volume);
            std::thread::spawn(move || {
                Engine::new(cmd_rx, tx, sh, cx, vol).run();
            });
        }
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            cfg,
            tracks: HashMap::new(),
            meta: HashMap::new(),
            selected,
            renaming: None,
            rename_focus: false,
            search: String::new(),
            show_folders: false,
            cmd_tx,
            shared,
            snap: Snap::default(),
            cover: None,
            cover_gen: 0,
            shuffle: false,
            repeat: Repeat::Off,
            muted: false,
            seek_val: 0.0,
            seeking: false,
            watcher: None,
            fs_tx,
            fs_rx,
            dirty_since: None,
            scan_rx: None,
        };
        if !app.cfg.roots.is_empty() {
            app.restart_watcher();
            app.start_scan();
        }
        app
    }

    fn save_config(&self) {
        let p = config_path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(&self.cfg) {
            let _ = std::fs::write(p, s);
        }
    }

    fn next_id(&mut self) -> u64 {
        self.cfg.next_id += 1;
        self.cfg.next_id
    }

    // ── biblioteca ──
    fn restart_watcher(&mut self) {
        self.watcher = None;
        if self.cfg.roots.is_empty() {
            return;
        }
        let tx = self.fs_tx.clone();
        let ctx = self.ctx.clone();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if !matches!(ev.kind, notify::EventKind::Access(_)) {
                    let _ = tx.send(());
                    ctx.request_repaint();
                }
            }
        });
        if let Ok(mut w) = w {
            for r in &self.cfg.roots {
                let _ = w.watch(r, RecursiveMode::Recursive);
            }
            self.watcher = Some(w);
        }
    }

    fn start_scan(&mut self) {
        let (tx, rx) = mpsc::channel();
        let roots = self.cfg.roots.clone();
        let known: HashSet<PathBuf> = self.meta.keys().cloned().collect();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let folders = scan_roots(&roots);
            let mut todo: Vec<PathBuf> = vec![];
            for v in folders.values() {
                for p in v {
                    if !known.contains(p) {
                        todo.push(p.clone());
                    }
                }
            }
            let _ = tx.send(ScanMsg::Folders(folders));
            ctx.request_repaint();
            for chunk in todo.chunks(40) {
                let batch: Vec<(PathBuf, Meta)> = chunk.iter().map(|p| (p.clone(), read_info(p, None, false).0)).collect();
                if tx.send(ScanMsg::Metas(batch)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
            let _ = tx.send(ScanMsg::Done);
            ctx.request_repaint();
        });
        self.scan_rx = Some(rx);
    }

    fn apply_scan(&mut self, map: HashMap<PathBuf, Vec<PathBuf>>) {
        let roots = self.cfg.roots.clone();
        self.cfg.playlists.retain(|p| match &p.source {
            PlaylistSource::Folder(path) => path.exists() && roots.iter().any(|r| path.starts_with(r)),
            _ => true,
        });
        let mut keys: Vec<PathBuf> = map.keys().cloned().collect();
        keys.sort();
        for k in keys {
            let exists = self.cfg.playlists.iter().any(|p| matches!(&p.source, PlaylistSource::Folder(x) if *x == k));
            if !exists {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist { id, name: folder_name(&k), source: PlaylistSource::Folder(k) });
            }
        }
        self.tracks = map;
        if self.selected.map_or(true, |s| !self.cfg.playlists.iter().any(|p| p.id == s)) {
            self.selected = self.cfg.playlists.first().map(|p| p.id);
        }
        self.save_config();
    }

    fn tracks_of(&self, id: u64) -> Vec<PathBuf> {
        match self.cfg.playlists.iter().find(|p| p.id == id).map(|p| &p.source) {
            Some(PlaylistSource::Folder(path)) => self.tracks.get(path).cloned().unwrap_or_default(),
            Some(PlaylistSource::Custom(v)) => v.clone(),
            None => vec![],
        }
    }

    // ── reprodução ──
    fn eff_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.cfg.volume }
    }

    fn play_playlist(&mut self, id: u64, idx: usize) {
        let tr = self.tracks_of(id);
        if tr.is_empty() {
            return;
        }
        let _ = self.cmd_tx.send(Cmd::PlayQueue { queue: tr, index: idx, playlist: Some(id) });
    }

    // ── tick por frame ──
    fn tick(&mut self, ctx: &egui::Context) {
        while self.fs_rx.try_recv().is_ok() {
            self.dirty_since = Some(Instant::now());
        }
        if let Some(t) = self.dirty_since {
            if t.elapsed() >= Duration::from_millis(700) {
                self.dirty_since = None;
                self.start_scan();
            } else {
                ctx.request_repaint_after(Duration::from_millis(300));
            }
        }
        // Mensagens do scan em segundo plano
        let mut msgs: Vec<ScanMsg> = vec![];
        let mut gone = false;
        if let Some(rx) = &self.scan_rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(_) => {
                        gone = true;
                        break;
                    }
                }
            }
        }
        for m in msgs {
            match m {
                ScanMsg::Folders(f) => self.apply_scan(f),
                ScanMsg::Metas(v) => {
                    for (p, mt) in v {
                        self.meta.insert(p, mt);
                    }
                }
                ScanMsg::Done => self.scan_rx = None,
            }
        }
        if gone {
            self.scan_rx = None;
        }
        // Estado do motor de áudio (que roda em outra thread)
        let mut new_cover: Option<Option<Vec<u8>>> = None;
        {
            let g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            self.snap = Snap {
                now: g.now.clone(),
                active: g.active,
                paused: g.paused,
                pos: g.pos,
                cur_path: g.cur_path.clone(),
                queue_pl: g.queue_pl,
                has_queue: g.has_queue,
                status: g.status.clone(),
            };
            if g.cover_gen != self.cover_gen {
                self.cover_gen = g.cover_gen;
                new_cover = Some(g.cover.clone());
            }
        }
        if let Some(c) = new_cover {
            self.cover = c.and_then(|b| make_texture(&self.ctx, &b));
        }
        if !self.seeking {
            self.seek_val = self.snap.pos;
        }
        if self.snap.active && !self.snap.paused {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // Espaço = play/pause
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.handle(Act::Toggle);
        }
        // Arrastar arquivos para uma playlist personalizada
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            let mut files = vec![];
            for p in dropped {
                if p.is_dir() {
                    collect_audio(&p, &mut files, 0);
                } else if is_audio(&p) {
                    files.push(p);
                }
            }
            self.add_files_to_selected(files);
        }
    }

    fn add_files_to_selected(&mut self, files: Vec<PathBuf>) {
        let Some(sel) = self.selected else { return };
        let mut changed = false;
        if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == sel) {
            if let PlaylistSource::Custom(v) = &mut pl.source {
                for f in files {
                    if !v.contains(&f) {
                        v.push(f);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.save_config();
        }
    }

    // ── ações ──
    fn handle(&mut self, act: Act) {
        match act {
            Act::Select(id) => {
                self.selected = Some(id);
                self.search.clear();
            }
            Act::StartRename(id) => {
                if let Some(p) = self.cfg.playlists.iter().find(|p| p.id == id) {
                    self.renaming = Some((id, p.name.clone()));
                    self.rename_focus = true;
                }
            }
            Act::CommitRename => {
                if let Some((id, buf)) = self.renaming.take() {
                    let n = buf.trim().to_string();
                    if !n.is_empty() {
                        if let Some(p) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
                            p.name = n;
                        }
                        self.save_config();
                    }
                }
            }
            Act::CancelRename => self.renaming = None,
            Act::MoveUp(i) => {
                if i > 0 && i < self.cfg.playlists.len() {
                    self.cfg.playlists.swap(i, i - 1);
                    self.save_config();
                }
            }
            Act::MoveDown(i) => {
                if i + 1 < self.cfg.playlists.len() {
                    self.cfg.playlists.swap(i, i + 1);
                    self.save_config();
                }
            }
            Act::Delete(id) => {
                self.cfg.playlists.retain(|p| p.id != id);
                if self.selected == Some(id) {
                    self.selected = self.cfg.playlists.first().map(|p| p.id);
                }
                self.save_config();
            }
            Act::ResetName(id) => {
                if let Some(p) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
                    if let PlaylistSource::Folder(path) = &p.source {
                        p.name = folder_name(path);
                    }
                }
                self.save_config();
            }
            Act::NewPlaylist => {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist { id, name: "Nova playlist".into(), source: PlaylistSource::Custom(vec![]) });
                self.selected = Some(id);
                self.search.clear();
                self.save_config();
                self.handle(Act::StartRename(id));
            }
            Act::NewWith(path) => {
                let id = self.next_id();
                self.cfg.playlists.push(Playlist { id, name: "Nova playlist".into(), source: PlaylistSource::Custom(vec![path]) });
                self.save_config();
            }
            Act::AddFolders => {
                if let Some(dirs) = rfd::FileDialog::new().set_title("Escolha a pasta das músicas").pick_folders() {
                    for d in dirs {
                        if !self.cfg.roots.contains(&d) {
                            self.cfg.roots.push(d);
                        }
                    }
                    self.save_config();
                    self.restart_watcher();
                    self.start_scan();
                }
            }
            Act::RemoveRoot(i) => {
                if i < self.cfg.roots.len() {
                    self.cfg.roots.remove(i);
                    self.save_config();
                    self.restart_watcher();
                    self.start_scan();
                }
            }
            Act::Refresh => self.start_scan(),
            Act::PlayAt(i) => {
                if let Some(sel) = self.selected {
                    self.play_playlist(sel, i);
                }
            }
            Act::PlayAll => {
                if let Some(sel) = self.selected {
                    let n = self.tracks_of(sel).len();
                    let start = if self.shuffle && n > 1 { rand::thread_rng().gen_range(0..n) } else { 0 };
                    self.play_playlist(sel, start);
                }
            }
            Act::AddTo(id, path) => {
                if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == id) {
                    if let PlaylistSource::Custom(v) = &mut pl.source {
                        if !v.contains(&path) {
                            v.push(path);
                        }
                    }
                }
                self.save_config();
            }
            Act::RemoveTrack(i) => {
                if let Some(sel) = self.selected {
                    if let Some(pl) = self.cfg.playlists.iter_mut().find(|p| p.id == sel) {
                        if let PlaylistSource::Custom(v) = &mut pl.source {
                            if i < v.len() {
                                v.remove(i);
                            }
                        }
                    }
                    self.save_config();
                }
            }
            Act::AddFiles => {
                if let Some(files) = rfd::FileDialog::new().set_title("Adicionar músicas").add_filter("Áudio", AUDIO_EXT).pick_files() {
                    self.add_files_to_selected(files);
                }
            }
            Act::Toggle => {
                if self.snap.active || self.snap.has_queue {
                    let _ = self.cmd_tx.send(Cmd::Toggle);
                } else if let Some(sel) = self.selected {
                    self.play_playlist(sel, 0);
                }
            }
            Act::Next => {
                let _ = self.cmd_tx.send(Cmd::Next);
            }
            Act::Prev => {
                let _ = self.cmd_tx.send(Cmd::Prev);
            }
            Act::Seek(t) => {
                let _ = self.cmd_tx.send(Cmd::Seek(t));
            }
            Act::VolChanged => {
                let v = self.eff_volume();
                let _ = self.cmd_tx.send(Cmd::Volume(v));
            }
        }
    }

    // ───────────────────────── UI: barra do player ─────────────────────────
    fn ui_player(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::TopBottomPanel::bottom("player")
            .exact_height(98.0)
            .frame(Frame::none().fill(PANEL).inner_margin(Margin::symmetric(20.0, 12.0)))
            .show(ctx, |ui| {
                let total = ui.available_width();
                let left_w = 300.0;
                let right_w = 190.0;
                let mid_w = (total - left_w - right_w - 16.0).max(240.0);
                ui.horizontal(|ui| {
                    // ── esquerda: capa + faixa atual
                    ui.allocate_ui_with_layout(Vec2::new(left_w, 72.0), Layout::left_to_right(Align::Center), |ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                        if let Some(tex) = &self.cover {
                            ui.put(
                                r,
                                egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
                                    .fit_to_exact_size(Vec2::splat(64.0))
                                    .rounding(10.0),
                            );
                        } else if let Some(n) = &self.snap.now {
                            let (c1, c2) = tile_colors(&n.title);
                            rounded_gradient(ui.painter(), r, 10.0, c1, c2);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, initial(&n.title), FontId::proportional(28.0), Color32::from_white_alpha(210));
                        } else {
                            rounded_gradient(ui.painter(), r, 10.0, BTN, PANEL);
                        }
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.set_max_width(left_w - 84.0);
                            if let Some(n) = &self.snap.now {
                                ui.add(egui::Label::new(RichText::new(&n.title).size(15.5).strong().color(Color32::WHITE)).wrap_mode(egui::TextWrapMode::Truncate));
                                let a = if n.artist.is_empty() { "Artista desconhecido" } else { n.artist.as_str() };
                                ui.add(egui::Label::new(RichText::new(a).size(12.5).color(MUTED)).wrap_mode(egui::TextWrapMode::Truncate));
                            } else {
                                ui.label(RichText::new("Nada tocando").size(15.0).color(MUTED));
                                ui.label(RichText::new("Escolha uma música").size(12.5).color(MUTED));
                            }
                        });
                    });
                    // ── centro: controles + linha do tempo
                    ui.allocate_ui_with_layout(Vec2::new(mid_w, 72.0), Layout::top_down(Align::LEFT), |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            ui.add_space(((mid_w - 230.0) / 2.0).max(0.0));
                            if icon_button(ui, Icon::Shuffle, 30.0, false, self.shuffle).on_hover_text("Aleatório").clicked() {
                                self.shuffle = !self.shuffle;
                                let _ = self.cmd_tx.send(Cmd::Shuffle(self.shuffle));
                            }
                            if icon_button(ui, Icon::Prev, 34.0, false, false).clicked() {
                                acts.push(Act::Prev);
                            }
                            let playing = self.snap.active && !self.snap.paused;
                            let ic = if playing { Icon::Pause } else { Icon::Play };
                            if icon_button(ui, ic, 46.0, true, false).clicked() {
                                acts.push(Act::Toggle);
                            }
                            if icon_button(ui, Icon::Next, 34.0, false, false).clicked() {
                                acts.push(Act::Next);
                            }
                            let (ri, tip) = match self.repeat {
                                Repeat::Off => (Icon::Repeat, "Repetir: desligado"),
                                Repeat::All => (Icon::Repeat, "Repetir: tudo"),
                                Repeat::One => (Icon::RepeatOne, "Repetir: uma música"),
                            };
                            if icon_button(ui, ri, 30.0, false, self.repeat != Repeat::Off).on_hover_text(tip).clicked() {
                                self.repeat = match self.repeat {
                                    Repeat::Off => Repeat::All,
                                    Repeat::All => Repeat::One,
                                    Repeat::One => Repeat::Off,
                                };
                                let _ = self.cmd_tx.send(Cmd::Repeat(self.repeat));
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let dur = self.snap.now.as_ref().map_or(0.0, |n| n.duration);
                            let max = dur.max(self.seek_val + 0.5).max(1.0);
                            ui.add_sized([36.0, 16.0], egui::Label::new(RichText::new(fmt_time(self.seek_val)).size(12.0).color(MUTED)));
                            let mut v = self.seek_val;
                            let out = bar(ui, &mut v, max, (mid_w - 36.0 * 2.0 - 16.0).max(60.0), self.snap.active);
                            self.seeking = out.dragging;
                            if out.changed {
                                self.seek_val = v;
                            }
                            if out.released {
                                acts.push(Act::Seek(v));
                            }
                            ui.add_sized([36.0, 16.0], egui::Label::new(RichText::new(fmt_time(dur)).size(12.0).color(MUTED)));
                        });
                    });
                    // ── direita: volume
                    ui.allocate_ui_with_layout(Vec2::new(right_w, 72.0), Layout::right_to_left(Align::Center), |ui| {
                        let mut changed = false;
                        let shown = if self.muted { 0.0 } else { self.cfg.volume };
                        let mut v = shown;
                        let out = bar(ui, &mut v, 1.0, 110.0, true);
                        if out.changed {
                            self.cfg.volume = v;
                            self.muted = false;
                            changed = true;
                        }
                        let ic = if self.muted || self.cfg.volume <= 0.001 { Icon::Muted } else { Icon::Volume };
                        if icon_button(ui, ic, 32.0, false, false).on_hover_text("Silenciar").clicked() {
                            self.muted = !self.muted;
                            changed = true;
                        }
                        if changed {
                            acts.push(Act::VolChanged);
                        }
                    });
                });
            });
    }

    // ───────────────────────── UI: barra lateral ─────────────────────────
    fn ui_sidebar(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::SidePanel::left("side")
            .exact_width(262.0)
            .resizable(false)
            .frame(Frame::none().fill(PANEL).inner_margin(Margin::symmetric(12.0, 16.0)))
            .show(ctx, |ui| {
                let time = ui.input(|i| i.time);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    rounded_gradient(ui.painter(), r, 11.0, ACCENT, CYAN);
                    let c = r.center();
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![Pos2::new(c.x - 4.5, c.y - 7.0), Pos2::new(c.x - 4.5, c.y + 7.0), Pos2::new(c.x + 8.0, c.y)],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Zek Music").size(22.0).strong().color(Color32::WHITE));
                });
                ui.add_space(18.0);
                ui.label(RichText::new("   SUAS PLAYLISTS").size(11.0).strong().color(MUTED));
                ui.add_space(6.0);
                let h = (ui.available_height() - 96.0).max(80.0);
                egui::ScrollArea::vertical().max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    for (i, p) in self.cfg.playlists.iter().enumerate() {
                        let selected = self.selected == Some(p.id);
                        let renaming_this = matches!(&self.renaming, Some((rid, _)) if *rid == p.id);
                        if renaming_this {
                            let focus = self.rename_focus;
                            self.rename_focus = false;
                            if let Some((_, buf)) = self.renaming.as_mut() {
                                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY));
                                if focus {
                                    r.request_focus();
                                }
                                if r.lost_focus() {
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        acts.push(Act::CancelRename);
                                    } else {
                                        acts.push(Act::CommitRename);
                                    }
                                }
                            }
                            continue;
                        }
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
                        let (count, custom) = match &p.source {
                            PlaylistSource::Folder(x) => (self.tracks.get(x).map_or(0, |v| v.len()), false),
                            PlaylistSource::Custom(v) => (v.len(), true),
                        };
                        let is_playing = self.snap.queue_pl == Some(p.id) && self.snap.active && !self.snap.paused;
                        {
                            let painter = ui.painter();
                            if selected {
                                painter.rect_filled(rect, 12.0, SEL_BG);
                                painter.rect_filled(Rect::from_min_size(rect.min + Vec2::new(0.0, 12.0), Vec2::new(3.0, rect.height() - 24.0)), 2.0, ACCENT);
                            } else if resp.hovered() {
                                painter.rect_filled(rect, 12.0, ROW_HOVER);
                            }
                            let tile = Rect::from_min_size(Pos2::new(rect.left() + 12.0, rect.center().y - 18.0), Vec2::splat(36.0));
                            let (c1, c2) = tile_colors(&p.id.to_string());
                            rounded_gradient(painter, tile, 9.0, c1, c2);
                            painter.text(tile.center(), Align2::CENTER_CENTER, initial(&p.name), FontId::proportional(16.0), Color32::from_white_alpha(225));
                            let right = if selected { rect.right() - 58.0 } else { rect.right() - 34.0 };
                            let clip = Rect::from_min_max(rect.min, Pos2::new(right, rect.max.y));
                            let pc = painter.with_clip_rect(clip);
                            pc.text(Pos2::new(rect.left() + 58.0, rect.center().y - 9.0), Align2::LEFT_CENTER, &p.name, FontId::proportional(14.5), if selected { Color32::WHITE } else { TEXT });
                            let sub = format!("{} músicas{}", count, if custom { " • personalizada" } else { "" });
                            pc.text(Pos2::new(rect.left() + 58.0, rect.center().y + 10.0), Align2::LEFT_CENTER, sub, FontId::proportional(11.5), MUTED);
                            if !selected && is_playing {
                                equalizer(painter, Pos2::new(rect.right() - 18.0, rect.center().y), time, true);
                            }
                        }
                        if selected {
                            let up = Rect::from_center_size(Pos2::new(rect.right() - 38.0, rect.center().y), Vec2::splat(22.0));
                            let dn = Rect::from_center_size(Pos2::new(rect.right() - 14.0, rect.center().y), Vec2::splat(22.0));
                            if arrow_btn(ui, up, true, ui.id().with(("up", p.id))).clicked() {
                                acts.push(Act::MoveUp(i));
                            }
                            if arrow_btn(ui, dn, false, ui.id().with(("dn", p.id))).clicked() {
                                acts.push(Act::MoveDown(i));
                            }
                        }
                        if resp.clicked() {
                            acts.push(Act::Select(p.id));
                        }
                        if resp.double_clicked() {
                            acts.push(Act::StartRename(p.id));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Renomear").clicked() {
                                acts.push(Act::StartRename(p.id));
                                ui.close_menu();
                            }
                            if ui.button("Mover para cima").clicked() {
                                acts.push(Act::MoveUp(i));
                                ui.close_menu();
                            }
                            if ui.button("Mover para baixo").clicked() {
                                acts.push(Act::MoveDown(i));
                                ui.close_menu();
                            }
                            match &p.source {
                                PlaylistSource::Custom(_) => {
                                    if ui.button("Excluir playlist").clicked() {
                                        acts.push(Act::Delete(p.id));
                                        ui.close_menu();
                                    }
                                }
                                PlaylistSource::Folder(_) => {
                                    if ui.button("Restaurar nome da pasta").clicked() {
                                        acts.push(Act::ResetName(p.id));
                                        ui.close_menu();
                                    }
                                }
                            }
                        });
                    }
                });
                ui.add_space(10.0);
                let w = ui.available_width();
                if ui.add_sized([w, 36.0], egui::Button::new(RichText::new("+  Nova playlist").color(Color32::WHITE).strong()).fill(ACCENT)).clicked() {
                    acts.push(Act::NewPlaylist);
                }
                if ui.add_sized([w, 36.0], egui::Button::new("Pastas da biblioteca")).clicked() {
                    self.show_folders = true;
                }
            });
    }

    // ───────────────────────── UI: área central ─────────────────────────
    fn ui_central(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        egui::CentralPanel::default().frame(Frame::none().fill(BG)).show(ctx, |ui| {
            let full = ui.max_rect();
            let gh = full.height().min(380.0);
            gradient_rect_v(ui.painter(), Rect::from_min_size(full.min, Vec2::new(full.width(), gh)), Color32::from_rgb(24, 54, 128), BG);

            let has_custom = self.cfg.playlists.iter().any(|p| matches!(p.source, PlaylistSource::Custom(_)));
            if self.cfg.roots.is_empty() && !has_custom {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.26);
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(84.0), Sense::hover());
                    rounded_gradient(ui.painter(), r, 24.0, ACCENT, CYAN);
                    let c = r.center();
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![Pos2::new(c.x - 10.0, c.y - 16.0), Pos2::new(c.x - 10.0, c.y + 16.0), Pos2::new(c.x + 18.0, c.y)],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                    ui.add_space(14.0);
                    ui.label(RichText::new("Zek Music").size(40.0).strong().color(Color32::WHITE));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Escolha a pasta que contém as suas pastas de músicas.\nCada subpasta vira uma playlist automaticamente.").size(16.0).color(MUTED));
                    ui.add_space(20.0);
                    let b = egui::Button::new(RichText::new("Escolher pasta de músicas").size(16.0).color(Color32::WHITE).strong())
                        .fill(ACCENT)
                        .min_size(Vec2::new(280.0, 46.0));
                    if ui.add(b).clicked() {
                        acts.push(Act::AddFolders);
                    }
                });
                return;
            }
            let Some(sel) = self.selected else {
                ui.label(RichText::new("Selecione uma playlist ao lado.").color(MUTED));
                return;
            };
            let Some(pl) = self.cfg.playlists.iter().find(|p| p.id == sel) else { return };
            let is_custom = matches!(pl.source, PlaylistSource::Custom(_));
            let name = pl.name.clone();
            let tracks: Vec<PathBuf> = match &pl.source {
                PlaylistSource::Folder(p) => self.tracks.get(p).cloned().unwrap_or_default(),
                PlaylistSource::Custom(v) => v.clone(),
            };
            let customs: Vec<(u64, String)> = self
                .cfg
                .playlists
                .iter()
                .filter(|p| matches!(p.source, PlaylistSource::Custom(_)) && p.id != sel)
                .map(|p| (p.id, p.name.clone()))
                .collect();
            let cur_path: Option<PathBuf> = if self.snap.queue_pl == Some(sel) && self.snap.active { self.snap.cur_path.clone() } else { None };
            let total_secs: f32 = tracks.iter().filter_map(|t| self.meta.get(t)).map(|m| m.duration).sum();
            let time_now = ui.input(|i| i.time);
            let playing_anim = self.snap.active && !self.snap.paused;

            Frame::none().inner_margin(Margin { left: 30.0, right: 30.0, top: 30.0, bottom: 0.0 }).show(ui, |ui| {
                // ── cabeçalho
                ui.horizontal(|ui| {
                    let (cr, _) = ui.allocate_exact_size(Vec2::splat(150.0), Sense::hover());
                    let (c1, c2) = tile_colors(&sel.to_string());
                    rounded_gradient(ui.painter(), cr, 18.0, c1, c2);
                    ui.painter().text(cr.center(), Align2::CENTER_CENTER, initial(&name), FontId::proportional(70.0), Color32::from_white_alpha(205));
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new(if is_custom { "PLAYLIST PERSONALIZADA" } else { "PLAYLIST" }).size(11.5).strong().color(ACCENT_HI));
                        let r = ui.add(
                            egui::Label::new(RichText::new(&name).size(42.0).strong().color(Color32::WHITE))
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .sense(Sense::click()),
                        );
                        if r.double_clicked() {
                            acts.push(Act::StartRename(sel));
                        }
                        r.on_hover_text("Duplo clique para renomear");
                        let mut sub = format!("{} músicas", tracks.len());
                        if total_secs > 60.0 {
                            sub.push_str(&format!("  •  {}", fmt_total(total_secs)));
                        }
                        if self.scan_rx.is_some() {
                            sub.push_str("  •  atualizando...");
                        }
                        ui.label(RichText::new(sub).size(14.0).color(MUTED));
                        if !self.snap.status.is_empty() {
                            ui.label(RichText::new(&self.snap.status).size(13.0).color(WARN));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            let same = self.snap.queue_pl == Some(sel) && self.snap.active;
                            let this_playing = same && !self.snap.paused;
                            let ic = if this_playing { Icon::Pause } else { Icon::Play };
                            if icon_button(ui, ic, 54.0, true, false).clicked() {
                                acts.push(if same { Act::Toggle } else { Act::PlayAll });
                            }
                            if icon_button(ui, Icon::Shuffle, 38.0, false, self.shuffle).on_hover_text("Aleatório").clicked() {
                                self.shuffle = !self.shuffle;
                                let _ = self.cmd_tx.send(Cmd::Shuffle(self.shuffle));
                            }
                            if is_custom && ui.add(egui::Button::new("Adicionar músicas").min_size(Vec2::new(150.0, 34.0))).clicked() {
                                acts.push(Act::AddFiles);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Buscar nesta playlist...").desired_width(230.0).margin(Vec2::new(10.0, 8.0)));
                            });
                        });
                    });
                });
                ui.add_space(22.0);

                if tracks.is_empty() {
                    ui.add_space(24.0);
                    let msg = if is_custom {
                        "Playlist vazia. Use \"Adicionar músicas\", arraste arquivos para cá\nou clique com o botão direito numa música de outra playlist."
                    } else {
                        "Nenhuma música encontrada nesta pasta."
                    };
                    ui.label(RichText::new(msg).size(15.0).color(MUTED));
                    return;
                }

                // ── cabeçalho da tabela
                let (hr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                {
                    let (tx, ax, dx) = columns(hr);
                    let p = ui.painter();
                    let f = FontId::proportional(11.5);
                    p.text(Pos2::new(hr.left() + 24.0, hr.center().y), Align2::CENTER_CENTER, "#", f.clone(), MUTED);
                    p.text(Pos2::new(tx, hr.center().y), Align2::LEFT_CENTER, "TÍTULO", f.clone(), MUTED);
                    p.text(Pos2::new(ax, hr.center().y), Align2::LEFT_CENTER, "ARTISTA", f.clone(), MUTED);
                    p.text(Pos2::new(dx, hr.center().y), Align2::RIGHT_CENTER, "DURAÇÃO", f, MUTED);
                    p.hline(hr.x_range(), hr.bottom(), stroke(1.0, Color32::from_rgb(30, 44, 84)));
                }
                ui.add_space(4.0);

                let q = self.search.trim().to_lowercase();
                let filtered: Vec<usize> = if q.is_empty() {
                    (0..tracks.len()).collect()
                } else {
                    (0..tracks.len())
                        .filter(|&i| {
                            let t = &tracks[i];
                            let mut hay = clean_title(&stem(t)).to_lowercase();
                            if let Some(m) = self.meta.get(t) {
                                hay.push(' ');
                                hay.push_str(&m.title.to_lowercase());
                                hay.push(' ');
                                hay.push_str(&m.artist.to_lowercase());
                            }
                            hay.contains(&q)
                        })
                        .collect()
                };

                let row_h = 46.0;
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, filtered.len(), |ui, range| {
                    for fi in range {
                        let ti = filtered[fi];
                        let path = &tracks[ti];
                        let playing = cur_path.as_ref() == Some(path);
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                        let m = self.meta.get(path);
                        let title = m.map(|m| m.title.clone()).unwrap_or_else(|| clean_title(&stem(path)));
                        let artist = m.map(|m| m.artist.clone()).unwrap_or_default();
                        let dur = m.map_or(0.0, |m| m.duration);
                        {
                            let painter = ui.painter();
                            if playing {
                                painter.rect_filled(rect, 10.0, SEL_BG);
                            } else if resp.hovered() {
                                painter.rect_filled(rect, 10.0, ROW_HOVER);
                            }
                            let cy = rect.center().y;
                            let (tx, ax, dx) = columns(rect);
                            if playing {
                                equalizer(painter, Pos2::new(rect.left() + 24.0, cy), time_now, playing_anim);
                            } else if resp.hovered() {
                                painter.add(egui::Shape::convex_polygon(
                                    vec![Pos2::new(rect.left() + 20.0, cy - 6.0), Pos2::new(rect.left() + 20.0, cy + 6.0), Pos2::new(rect.left() + 30.0, cy)],
                                    Color32::WHITE,
                                    Stroke::NONE,
                                ));
                            } else {
                                painter.text(Pos2::new(rect.left() + 24.0, cy), Align2::CENTER_CENTER, (ti + 1).to_string(), FontId::proportional(13.0), MUTED);
                            }
                            let tclip = Rect::from_min_max(rect.min, Pos2::new((ax - 14.0).max(tx), rect.max.y));
                            painter.with_clip_rect(tclip).text(Pos2::new(tx, cy), Align2::LEFT_CENTER, title, FontId::proportional(15.0), if playing { ACCENT_HI } else { TEXT });
                            if !artist.is_empty() {
                                let aclip = Rect::from_min_max(Pos2::new(ax, rect.min.y), Pos2::new((dx - 64.0).max(ax), rect.max.y));
                                painter.with_clip_rect(aclip).text(Pos2::new(ax, cy), Align2::LEFT_CENTER, artist, FontId::proportional(13.5), MUTED);
                            }
                            if dur > 0.0 {
                                painter.text(Pos2::new(dx, cy), Align2::RIGHT_CENTER, fmt_time(dur), FontId::proportional(13.5), MUTED);
                            }
                        }
                        if resp.clicked() {
                            acts.push(Act::PlayAt(ti));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Tocar").clicked() {
                                acts.push(Act::PlayAt(ti));
                                ui.close_menu();
                            }
                            ui.menu_button("Adicionar à playlist", |ui| {
                                for (id, n) in &customs {
                                    if ui.button(n).clicked() {
                                        acts.push(Act::AddTo(*id, path.clone()));
                                        ui.close_menu();
                                    }
                                }
                                if ui.button("+ Nova playlist").clicked() {
                                    acts.push(Act::NewWith(path.clone()));
                                    ui.close_menu();
                                }
                            });
                            if is_custom && ui.button("Remover desta playlist").clicked() {
                                acts.push(Act::RemoveTrack(ti));
                                ui.close_menu();
                            }
                        });
                    }
                });
            });
        });
    }

    // ───────────────────────── UI: janela de pastas ─────────────────────────
    fn ui_folders(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        if !self.show_folders {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new("Pastas da biblioteca").strong())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.label(RichText::new("Escolha a pasta que CONTÉM as suas pastas de músicas (por exemplo, \"musicas\"). Cada subpasta vira uma playlist.").color(MUTED));
                ui.add_space(10.0);
                if self.cfg.roots.is_empty() {
                    ui.label(RichText::new("Nenhuma pasta adicionada ainda.").color(MUTED));
                }
                for (i, r) in self.cfg.roots.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button("Remover").clicked() {
                            acts.push(Act::RemoveRoot(i));
                        }
                        ui.add(egui::Label::new(RichText::new(r.to_string_lossy()).color(TEXT)).wrap_mode(egui::TextWrapMode::Truncate));
                    });
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Adicionar pasta").color(Color32::WHITE).strong()).fill(ACCENT)).clicked() {
                        acts.push(Act::AddFolders);
                    }
                    if ui.button("Atualizar biblioteca").clicked() {
                        acts.push(Act::Refresh);
                    }
                });
            });
        self.show_folders = open;
    }
}

impl eframe::App for ZekApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
        let mut acts: Vec<Act> = vec![];
        self.ui_player(ctx, &mut acts);
        self.ui_sidebar(ctx, &mut acts);
        self.ui_central(ctx, &mut acts);
        self.ui_folders(ctx, &mut acts);
        for a in acts {
            self.handle(a);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config();
    }
}

// ───────────────────────── Estilo ─────────────────────────
fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(7, 11, 22);
    v.faint_bg_color = PANEL;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = stroke(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT_HI;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.fg_stroke = stroke(1.0, TEXT);
    v.widgets.noninteractive.bg_stroke = stroke(1.0, Color32::from_rgb(24, 34, 62));
    v.widgets.inactive.bg_fill = BTN;
    v.widgets.inactive.weak_bg_fill = BTN;
    v.widgets.inactive.fg_stroke = stroke(1.0, TEXT);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = BTN_HOVER;
    v.widgets.hovered.weak_bg_fill = BTN_HOVER;
    v.widgets.hovered.fg_stroke = stroke(1.0, Color32::WHITE);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.widgets.active.fg_stroke = stroke(1.0, Color32::WHITE);
    let r = Rounding::same(10.0);
    v.widgets.noninteractive.rounding = r;
    v.widgets.inactive.rounding = r;
    v.widgets.hovered.rounding = r;
    v.widgets.active.rounding = r;
    v.widgets.open.rounding = r;
    v.window_rounding = Rounding::same(14.0);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 7.0);
    });
}

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../packaging/zek-music.png")).ok()?;
    let img = img.thumbnail(256, 256).to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() -> eframe::Result<()> {
    let mut vp = egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 740.0])
        .with_min_inner_size([980.0, 580.0])
        .with_title("Zek Music")
        .with_app_id("zek-music");
    if let Some(ic) = load_icon() {
        vp = vp.with_icon(ic);
    }
    let opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native("Zek Music", opts, Box::new(|cc| Ok(Box::new(ZekApp::new(cc)))))
}
